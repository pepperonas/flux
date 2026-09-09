use crate::core::ids::ModuleId;
use crate::graph::module::{ModuleSpec, Zone};
use crate::graph::signal::{can_connect, SignalType};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModuleKind {
    // Voice zone: specs only in M1a. The voice chain is executed directly in
    // audio::voice so that milestone M1a does not need per-voice graph
    // machinery; the specs exist so the patch view can already draw the chain,
    // and so M4 can make it editable without redefining the contract.
    Oscillator,
    Filter,
    Envelope,
    Vca,
    // Global zone: executed through the schedule.
    Lfo,
    Mixer,
    Delay,
    Reverb,
    Output,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PortRef {
    pub module: ModuleId,
    pub port: u8,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Edge {
    pub from: PortRef,
    pub to: PortRef,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Node {
    pub id: ModuleId,
    pub kind: ModuleKind,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
pub enum PatchError {
    #[error("no module with id {0:?}")]
    UnknownModule(ModuleId),
    #[error("module {module:?} has no port {port}")]
    UnknownPort { module: ModuleId, port: u8 },
    #[error("cannot connect {from:?} to {to:?}")]
    TypeMismatch { from: SignalType, to: SignalType },
    #[error("that input already has a connection")]
    InputAlreadyConnected { module: ModuleId, port: u8 },
    #[error("that connection would create a loop")]
    WouldCycle,
}

#[derive(Clone, Default, Debug)]
pub struct Patch {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    next_id: u16,
}

impl Patch {
    pub fn add(&mut self, kind: ModuleKind) -> ModuleId {
        let id = ModuleId(self.next_id);
        self.next_id += 1;
        self.nodes.push(Node { id, kind });
        id
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    pub fn kind(&self, id: ModuleId) -> Option<ModuleKind> {
        self.nodes.iter().find(|n| n.id == id).map(|n| n.kind)
    }

    fn spec(&self, id: ModuleId) -> Result<&'static ModuleSpec, PatchError> {
        self.kind(id)
            .map(spec_for)
            .ok_or(PatchError::UnknownModule(id))
    }

    pub fn connect(&mut self, from: PortRef, to: PortRef) -> Result<(), PatchError> {
        let from_spec = self.spec(from.module)?;
        let to_spec = self.spec(to.module)?;

        let out = from_spec
            .outputs
            .get(from.port as usize)
            .ok_or(PatchError::UnknownPort {
                module: from.module,
                port: from.port,
            })?;
        let inp = to_spec
            .inputs
            .get(to.port as usize)
            .ok_or(PatchError::UnknownPort {
                module: to.module,
                port: to.port,
            })?;

        if !can_connect(out.signal, inp.signal) {
            return Err(PatchError::TypeMismatch {
                from: out.signal,
                to: inp.signal,
            });
        }

        if self.edges.iter().any(|e| e.to == to) {
            return Err(PatchError::InputAlreadyConnected {
                module: to.module,
                port: to.port,
            });
        }

        // Check for a cycle before committing, so the refusal reaches the player
        // while their hand is still on the cable.
        self.edges.push(Edge { from, to });
        if self.topological_order().is_err() {
            self.edges.pop();
            return Err(PatchError::WouldCycle);
        }
        Ok(())
    }

    pub fn disconnect(&mut self, to: PortRef) {
        self.edges.retain(|e| e.to != to);
    }

    /// Kahn's algorithm. An orphan module still gets a turn - it may be a source
    /// nobody has patched yet, and skipping it would freeze its internal state.
    pub fn topological_order(&self) -> Result<Vec<ModuleId>, PatchError> {
        let mut incoming: Vec<usize> = self
            .nodes
            .iter()
            .map(|n| self.edges.iter().filter(|e| e.to.module == n.id).count())
            .collect();

        let mut ready: Vec<ModuleId> = self
            .nodes
            .iter()
            .zip(&incoming)
            .filter(|(_, c)| **c == 0)
            .map(|(n, _)| n.id)
            .collect();

        let index_of = |id: ModuleId| self.nodes.iter().position(|n| n.id == id);
        let mut order = Vec::with_capacity(self.nodes.len());

        while let Some(id) = ready.pop() {
            order.push(id);
            for edge in self.edges.iter().filter(|e| e.from.module == id) {
                if let Some(i) = index_of(edge.to.module) {
                    incoming[i] -= 1;
                    if incoming[i] == 0 {
                        ready.push(self.nodes[i].id);
                    }
                }
            }
        }

        if order.len() == self.nodes.len() {
            Ok(order)
        } else {
            Err(PatchError::WouldCycle)
        }
    }
}

macro_rules! ports {
    ($($name:literal : $sig:ident),* $(,)?) => {
        &[$(crate::graph::module::PortSpec {
            name: $name,
            signal: crate::graph::signal::SignalType::$sig,
        }),*]
    };
}

pub fn spec_for(kind: ModuleKind) -> &'static ModuleSpec {
    use crate::params::registry as p;
    match kind {
        ModuleKind::Oscillator => &ModuleSpec {
            name: "OSCILLATOR",
            zone: Zone::Voice,
            inputs: ports!("pitch": Control),
            outputs: ports!("audio": Audio),
            params: &[p::OSC_WAVE, p::OSC_DETUNE, p::OSC_LEVEL],
        },
        ModuleKind::Filter => &ModuleSpec {
            name: "FILTER",
            zone: Zone::Voice,
            inputs: ports!("audio": Audio, "cutoff": Control),
            outputs: ports!("audio": Audio),
            params: &[p::FILTER_CUTOFF, p::FILTER_RESONANCE],
        },
        ModuleKind::Envelope => &ModuleSpec {
            name: "ENVELOPE",
            zone: Zone::Voice,
            inputs: ports!("gate": Gate),
            outputs: ports!("level": Control),
            params: &[p::ENV_ATTACK, p::ENV_DECAY, p::ENV_SUSTAIN, p::ENV_RELEASE],
        },
        ModuleKind::Vca => &ModuleSpec {
            name: "VCA",
            zone: Zone::Voice,
            inputs: ports!("audio": Audio, "level": Control),
            outputs: ports!("audio": Audio),
            params: &[],
        },
        ModuleKind::Lfo => &ModuleSpec {
            name: "LFO",
            zone: Zone::Global,
            inputs: ports!(),
            outputs: ports!("mod": Control),
            params: &[p::LFO_RATE, p::LFO_AMOUNT],
        },
        ModuleKind::Mixer => &ModuleSpec {
            name: "MIXER",
            zone: Zone::Global,
            inputs: ports!("voices": Audio, "aux": Audio),
            outputs: ports!("audio": Audio),
            params: &[],
        },
        ModuleKind::Delay => &ModuleSpec {
            name: "DELAY",
            zone: Zone::Global,
            inputs: ports!("audio": Audio),
            outputs: ports!("audio": Audio),
            params: &[p::DELAY_TIME, p::DELAY_FEEDBACK, p::DELAY_MIX],
        },
        ModuleKind::Reverb => &ModuleSpec {
            name: "REVERB",
            zone: Zone::Global,
            inputs: ports!("audio": Audio),
            outputs: ports!("audio": Audio),
            params: &[p::REVERB_SIZE, p::REVERB_MIX],
        },
        // The terminal node's finished audio has to reach the device. Giving it
        // a real output port (rather than none) puts that signal in an ordinary
        // pool buffer the engine can read like any other module's output,
        // instead of needing to downcast a `Box<dyn Module>` through `Any`.
        ModuleKind::Output => &ModuleSpec {
            name: "OUTPUT",
            zone: Zone::Global,
            inputs: ports!("audio": Audio),
            outputs: ports!("audio": Audio),
            params: &[p::MASTER_GAIN],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_valid_audio_connection_is_accepted() {
        let mut p = Patch::default();
        let mixer = p.add(ModuleKind::Mixer);
        let out = p.add(ModuleKind::Output);
        assert!(p
            .connect(
                PortRef {
                    module: mixer,
                    port: 0
                },
                PortRef {
                    module: out,
                    port: 0
                }
            )
            .is_ok());
        assert_eq!(p.edges().len(), 1);
    }

    #[test]
    fn a_type_mismatch_is_refused_with_the_types_named() {
        // "Verhindere ungueltige Verbindungen" - and say why, so the message is
        // usable rather than just a refusal.
        //
        // Deviation from the brief: the brief's version of this test wired the
        // LFO into OUTPUT and expected Control -> Audio to be a mismatch. But
        // `control_is_a_signal_too` (mandated verbatim) asserts the opposite:
        // `can_connect(Control, Audio)` must be `true` - a control signal is
        // allowed to feed an audio input, same as `Audio -> Control` is
        // allowed the other way. Under the module specs as given, no signal
        // reaching OUTPUT's audio input can ever mismatch (only Audio and
        // Control can produce it, and both are legal into Audio), so that
        // pairing cannot demonstrate a refusal at all. Envelope's `gate` input
        // is the only non-Audio, non-Control input in this spec set, so LFO
        // (Control) -> Envelope (Gate) is used instead: it is a real mismatch,
        // and it exercises the exact headline rule this task exists to
        // establish (Control cannot become a Gate).
        let mut p = Patch::default();
        let lfo = p.add(ModuleKind::Lfo);
        let env = p.add(ModuleKind::Envelope);
        let err = p
            .connect(
                PortRef {
                    module: lfo,
                    port: 0,
                },
                PortRef {
                    module: env,
                    port: 0,
                },
            )
            .unwrap_err();
        match err {
            PatchError::TypeMismatch { from, to } => {
                assert_eq!(from, SignalType::Control);
                assert_eq!(to, SignalType::Gate);
            }
            other => panic!("expected a type mismatch, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_port_is_refused() {
        let mut p = Patch::default();
        let out = p.add(ModuleKind::Output);
        let mixer = p.add(ModuleKind::Mixer);
        assert!(matches!(
            p.connect(
                PortRef {
                    module: mixer,
                    port: 99
                },
                PortRef {
                    module: out,
                    port: 0
                }
            ),
            Err(PatchError::UnknownPort { .. })
        ));
    }

    #[test]
    fn an_input_takes_only_one_connection() {
        // Summing happens in a mixer, explicitly. Letting two sources land on one
        // input would sum them invisibly, and the patch view would show two
        // cables into a socket that looks like it holds one.
        let mut p = Patch::default();
        let a = p.add(ModuleKind::Mixer);
        let b = p.add(ModuleKind::Mixer);
        let out = p.add(ModuleKind::Output);
        p.connect(
            PortRef { module: a, port: 0 },
            PortRef {
                module: out,
                port: 0,
            },
        )
        .unwrap();
        assert!(matches!(
            p.connect(
                PortRef { module: b, port: 0 },
                PortRef {
                    module: out,
                    port: 0
                }
            ),
            Err(PatchError::InputAlreadyConnected { .. })
        ));
    }

    #[test]
    fn a_cycle_is_refused_at_edit_time() {
        // Rejecting the connection is better than accepting it and discovering
        // at compile time that the graph cannot be ordered. The player finds out
        // while their hand is still on the cable.
        let mut p = Patch::default();
        let a = p.add(ModuleKind::Delay);
        let b = p.add(ModuleKind::Delay);
        p.connect(
            PortRef { module: a, port: 0 },
            PortRef { module: b, port: 0 },
        )
        .unwrap();
        assert!(matches!(
            p.connect(
                PortRef { module: b, port: 0 },
                PortRef { module: a, port: 0 }
            ),
            Err(PatchError::WouldCycle)
        ));
    }

    #[test]
    fn a_module_may_not_feed_itself() {
        let mut p = Patch::default();
        let d = p.add(ModuleKind::Delay);
        assert!(matches!(
            p.connect(
                PortRef { module: d, port: 0 },
                PortRef { module: d, port: 0 }
            ),
            Err(PatchError::WouldCycle)
        ));
    }

    #[test]
    fn topological_order_puts_producers_before_consumers() {
        let mut p = Patch::default();
        let mixer = p.add(ModuleKind::Mixer);
        let delay = p.add(ModuleKind::Delay);
        let out = p.add(ModuleKind::Output);
        p.connect(
            PortRef {
                module: mixer,
                port: 0,
            },
            PortRef {
                module: delay,
                port: 0,
            },
        )
        .unwrap();
        p.connect(
            PortRef {
                module: delay,
                port: 0,
            },
            PortRef {
                module: out,
                port: 0,
            },
        )
        .unwrap();

        let order = p.topological_order().unwrap();
        let pos = |m: ModuleId| order.iter().position(|x| *x == m).unwrap();
        assert!(pos(mixer) < pos(delay));
        assert!(pos(delay) < pos(out));
        assert_eq!(order.len(), 3);
    }

    #[test]
    fn unconnected_modules_still_appear_in_the_order() {
        // An orphan module must still be given a turn: it may be a source whose
        // output nobody has patched yet, and skipping it would freeze its state.
        let mut p = Patch::default();
        let _lonely = p.add(ModuleKind::Lfo);
        let out = p.add(ModuleKind::Output);
        let order = p.topological_order().unwrap();
        assert_eq!(order.len(), 2);
        assert!(order.contains(&out));
    }
}

use crate::core::ids::{MacroId, ModSourceId};

/// A macro moves several parameters at once, so the player steers the music
/// instead of adjusting it.
///
/// A macro may be bipolar. The brief's names come in opposing pairs, and endless
/// encoders have no end stop, so a centred axis is the control they are
/// physically built for. All eight names survive; four are the negative half of
/// an axis.
#[derive(Clone, Copy, Debug)]
pub struct MacroDef {
    pub id: MacroId,
    pub name: &'static str,
    pub name_neg: Option<&'static str>,
    pub bipolar: bool,
    pub help: &'static str,
}

impl MacroDef {
    pub fn label(&self, value: f32) -> &'static str {
        match self.name_neg {
            Some(neg) if value < 0.0 => neg,
            _ => self.name,
        }
    }
}

pub const MACRO_COUNT: usize = 8;

pub const MACROS: [MacroDef; MACRO_COUNT] = [
    MacroDef { id: MacroId(0), name: "BRIGHT", name_neg: Some("DARK"), bipolar: true,
        help: "Opens or closes the sound. Right is brighter and more present, left is darker and further away." },
    MacroDef { id: MacroId(1), name: "WET", name_neg: Some("DRY"), bipolar: true,
        help: "How much space is around the sound. Right adds echo and room, left brings it close and direct." },
    MacroDef { id: MacroId(2), name: "ENERGY", name_neg: None, bipolar: false,
        help: "How hard the music pushes. Raises level, attack and movement together." },
    MacroDef { id: MacroId(3), name: "CHAOS", name_neg: None, bipolar: false,
        help: "How unpredictable things get. Increases variation in patterns, modulation and effects." },
    MacroDef { id: MacroId(4), name: "DENSITY", name_neg: None, bipolar: false,
        help: "How much is happening. More notes, more hits, more layers." },
    MacroDef { id: MacroId(5), name: "SPACE", name_neg: None, bipolar: false,
        help: "How wide and distant everything sits." },
    MacroDef { id: MacroId(6), name: "TIGHT", name_neg: Some("LOOSE"), bipolar: true,
        help: "How strictly things sit on the beat. Right is locked, left breathes." },
    MacroDef { id: MacroId(7), name: "SIMPLE", name_neg: Some("COMPLEX"), bipolar: true,
        help: "How intricate the material is. Right is elaborate, left is stripped back." },
];

/// Macros occupy the first modulation source slots. LFOs and mapped controls
/// take the ones after them.
pub fn macro_source(id: MacroId) -> ModSourceId {
    ModSourceId(id.0 as u16)
}

pub const LFO_SOURCE_BASE: u16 = MACRO_COUNT as u16;
pub const MOD_SOURCE_COUNT: usize = MACRO_COUNT + 4;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_are_eight_macros_for_eight_encoders() {
        assert_eq!(MACROS.len(), 8);
        assert_eq!(MACRO_COUNT, 8);
    }

    #[test]
    fn macro_ids_match_their_position() {
        for (i, m) in MACROS.iter().enumerate() {
            assert_eq!(m.id.0 as usize, i);
        }
    }

    #[test]
    fn every_name_from_the_brief_is_present() {
        // The brief names these explicitly. Losing one to a redesign should
        // break the build, not go unnoticed.
        let names: Vec<&str> = MACROS
            .iter()
            .flat_map(|m| [Some(m.name), m.name_neg].into_iter().flatten())
            .collect();
        for required in [
            "BRIGHT", "DARK", "WET", "DRY", "ENERGY", "CHAOS", "DENSITY", "SPACE",
        ] {
            assert!(
                names.contains(&required),
                "macro name {required} is missing"
            );
        }
    }

    #[test]
    fn a_bipolar_macro_shows_the_other_name_when_negative() {
        let bright = &MACROS[0];
        assert!(bright.bipolar);
        assert_eq!(bright.label(0.6), "BRIGHT");
        assert_eq!(bright.label(-0.6), "DARK");
        assert_eq!(bright.label(0.0), "BRIGHT");
    }

    #[test]
    fn a_unipolar_macro_keeps_its_name() {
        let chaos = MACROS.iter().find(|m| m.name == "CHAOS").unwrap();
        assert!(!chaos.bipolar);
        assert_eq!(chaos.label(0.0), "CHAOS");
        assert_eq!(chaos.label(1.0), "CHAOS");
    }

    #[test]
    fn macro_sources_do_not_collide() {
        use std::collections::HashSet;
        let ids: HashSet<_> = MACROS.iter().map(|m| macro_source(m.id)).collect();
        assert_eq!(ids.len(), MACROS.len());
    }
}

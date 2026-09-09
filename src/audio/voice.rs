use crate::audio::dsp::{Adsr, AdsrParams, Osc, Svf, Waveform};
use crate::core::music::note_to_freq;
use crate::params::registry::{self as p, ParamRegistry, PARAM_COUNT};

pub const VOICE_COUNT: usize = 16;

/// A block's resolved parameter values, normalised 0..1, indexed by `ParamId`.
/// A fixed array so it can be passed into the audio path without allocating.
#[derive(Clone, Copy)]
pub struct ParamValues(pub [f32; PARAM_COUNT]);

impl Default for ParamValues {
    fn default() -> Self {
        ParamValues([0.0; PARAM_COUNT])
    }
}

impl ParamValues {
    pub fn get(&self, id: crate::core::ids::ParamId) -> f32 {
        self.0[id.0 as usize]
    }
}

/// One sounding note.
///
/// The voice chain is written directly rather than executed through the graph.
/// Milestone M1a's patch is fixed, so per-voice graph machinery would be
/// complexity with no user-visible payoff; the module specs in `graph::patch`
/// describe this same chain, which is what the patch view draws and what M4
/// will make editable.
pub struct Voice {
    pub note: Option<u8>,
    /// Increments for every note started, so the oldest voice is identifiable.
    pub age: u64,
    velocity: f32,
    osc_a: Osc,
    osc_b: Osc,
    filter: Svf,
    env: Adsr,
    registry: ParamRegistry,
}

impl Default for Voice {
    fn default() -> Self {
        Voice {
            note: None,
            age: 0,
            velocity: 0.0,
            osc_a: Osc::default(),
            osc_b: Osc::default(),
            filter: Svf::default(),
            env: Adsr::default(),
            registry: ParamRegistry::new(),
        }
    }
}

impl Voice {
    pub fn start(&mut self, note: u8, velocity: f32, _sample_rate: f32) {
        self.note = Some(note);
        self.velocity = velocity.clamp(0.0, 1.0);
        // Oscillator phases are deliberately not reset. Restarting every voice
        // from phase zero makes stacked notes sum coherently on their first
        // cycle, which reads as a click at the start of a chord.
        self.filter.reset();
        self.env.gate_on();
    }

    pub fn release(&mut self) {
        self.env.gate_off();
    }

    pub fn is_idle(&self) -> bool {
        self.env.is_idle()
    }

    /// Adds this voice into `out`. Callers clear the buffer first.
    pub fn render(&mut self, out: &mut [f32], params: &ParamValues, sample_rate: f32) {
        let Some(note) = self.note else {
            return;
        };

        let wave = Waveform::from_index(
            self.registry
                .denormalize(p::OSC_WAVE, params.get(p::OSC_WAVE)),
        );
        let detune = self
            .registry
            .denormalize(p::OSC_DETUNE, params.get(p::OSC_DETUNE));
        let level = self
            .registry
            .denormalize(p::OSC_LEVEL, params.get(p::OSC_LEVEL));
        let cutoff = self
            .registry
            .denormalize(p::FILTER_CUTOFF, params.get(p::FILTER_CUTOFF));
        let resonance = self
            .registry
            .denormalize(p::FILTER_RESONANCE, params.get(p::FILTER_RESONANCE));
        let adsr = AdsrParams {
            attack: self
                .registry
                .denormalize(p::ENV_ATTACK, params.get(p::ENV_ATTACK)),
            decay: self
                .registry
                .denormalize(p::ENV_DECAY, params.get(p::ENV_DECAY)),
            sustain: self
                .registry
                .denormalize(p::ENV_SUSTAIN, params.get(p::ENV_SUSTAIN)),
            release: self
                .registry
                .denormalize(p::ENV_RELEASE, params.get(p::ENV_RELEASE)),
        };

        let base = note as f32;
        let freq_a = note_to_freq(base - detune / 100.0);
        let freq_b = note_to_freq(base + detune / 100.0);
        let amp = level * self.velocity;

        for sample in out.iter_mut() {
            let env = self.env.tick(&adsr, sample_rate);
            let raw = 0.5
                * (self.osc_a.tick(freq_a, sample_rate, wave)
                    + self.osc_b.tick(freq_b, sample_rate, wave));
            let filtered = self.filter.lowpass(raw, cutoff, resonance, sample_rate);
            *sample += filtered * env * amp;
        }

        if self.env.is_idle() {
            self.note = None;
        }
    }
}

pub struct VoicePool {
    pub voices: [Voice; VOICE_COUNT],
    next_age: u64,
}

impl Default for VoicePool {
    fn default() -> Self {
        VoicePool {
            voices: std::array::from_fn(|_| Voice::default()),
            next_age: 0,
        }
    }
}

impl VoicePool {
    /// Starts `note`. Returns the note of a voice that was stolen to make
    /// room, if stealing was necessary, so the caller can report it.
    pub fn note_on(&mut self, note: u8, velocity: f32, sample_rate: f32) -> Option<u8> {
        self.next_age += 1;

        // Retrigger a note that is already sounding rather than spending a
        // second voice on it - otherwise a repeating key eats the whole pool.
        if let Some(v) = self.voices.iter_mut().find(|v| v.note == Some(note)) {
            v.age = self.next_age;
            v.start(note, velocity, sample_rate);
            return None;
        }

        if let Some(v) = self.voices.iter_mut().find(|v| v.note.is_none()) {
            v.age = self.next_age;
            v.start(note, velocity, sample_rate);
            return None;
        }

        // Steal the oldest. It is released rather than cut, so the theft fades
        // instead of clicking.
        let victim = self
            .voices
            .iter_mut()
            .min_by_key(|v| v.age)
            .expect("the voice pool is never empty");
        let stolen = victim.note;
        victim.age = self.next_age;
        victim.start(note, velocity, sample_rate);
        stolen
    }

    pub fn note_off(&mut self, note: u8) {
        for v in self.voices.iter_mut().filter(|v| v.note == Some(note)) {
            v.release();
        }
    }

    pub fn active_count(&self) -> usize {
        self.voices.iter().filter(|v| v.note.is_some()).count()
    }

    /// Pitch classes currently sounding, one bit per semitone, for the interface.
    pub fn active_pitch_classes(&self) -> u16 {
        self.voices
            .iter()
            .filter_map(|v| v.note)
            .fold(0u16, |m, n| m | (1 << (n % 12)))
    }

    pub fn render(&mut self, out: &mut [f32], params: &ParamValues, sample_rate: f32) {
        for v in self.voices.iter_mut() {
            v.render(out, params, sample_rate);
        }
        // Sixteen voices at full level would sum to sixteen. Scaling by the
        // square root of the count keeps a chord roughly as loud as a single
        // note without ducking audibly as notes are added.
        let active = self.active_count().max(1) as f32;
        let scale = 1.0 / active.sqrt();
        for s in out.iter_mut() {
            *s *= scale;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn params() -> ParamValues {
        let reg = crate::params::registry::ParamRegistry::new();
        let mut v = ParamValues::default();
        for i in 0..crate::params::registry::PARAM_COUNT {
            let id = crate::core::ids::ParamId(i as u16);
            v.0[i] = reg.normalize(id, reg.desc(id).default);
        }
        v
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |a, b| a.max(b.abs()))
    }

    #[test]
    fn a_started_voice_makes_sound_and_an_idle_one_does_not() {
        let mut v = Voice::default();
        let mut buf = vec![0.0f32; 512];
        v.render(&mut buf, &params(), SR);
        assert_eq!(peak(&buf), 0.0, "an idle voice produced sound");

        v.start(60, 1.0, SR);
        buf.fill(0.0);
        v.render(&mut buf, &params(), SR);
        assert!(peak(&buf) > 0.01, "a started voice was silent");
    }

    #[test]
    fn a_released_voice_eventually_becomes_idle_and_free() {
        // Deviation from the brief: its version releases after one 512-frame
        // render and then renders 200 more (2.13 s total) before asserting
        // idle. That is not enough time for this voice to actually reach
        // idle. `gate_off` (Task 8, `Adsr::gate_off`) deliberately does not
        // reset the level to the sustain value - it releases from wherever
        // the envelope currently sits, which after one block is still
        // mid-decay at ~0.993, not the registered sustain default of 0.7.
        // From there, ENV_RELEASE's registered default of 0.4 s (Task 7,
        // `src/params/registry.rs`) and the release stage's exponential
        // coefficient need about 3.69 s (346 renders of 512 frames) to cross
        // `IDLE_THRESHOLD` (1e-4, Task 8, `src/audio/dsp.rs`) - confirmed by
        // simulating the exact per-sample recurrence in both f32 and f64. 200
        // renders only covers 2.13 s and leaves the level around 0.0048,
        // comfortably above the threshold, so the assertion below would fail
        // deterministically rather than flakily. This keeps the property
        // under test (a released voice eventually frees itself) but renders
        // long enough for that to actually happen, with headroom over the
        // computed 346.
        let mut v = Voice::default();
        v.start(60, 1.0, SR);
        let mut buf = vec![0.0f32; 512];
        v.render(&mut buf, &params(), SR);
        v.release();
        for _ in 0..400 {
            v.render(&mut buf, &params(), SR);
        }
        assert!(v.is_idle(), "the voice never freed itself");
        assert_eq!(v.note, None);
    }

    #[test]
    fn the_pool_gives_each_note_its_own_voice() {
        let mut pool = VoicePool::default();
        pool.note_on(60, 1.0, SR);
        pool.note_on(64, 1.0, SR);
        pool.note_on(67, 1.0, SR);
        assert_eq!(pool.active_count(), 3);
    }

    #[test]
    fn releasing_a_note_frees_only_that_voice() {
        let mut pool = VoicePool::default();
        pool.note_on(60, 1.0, SR);
        pool.note_on(64, 1.0, SR);
        pool.note_off(60);
        let mut buf = vec![0.0f32; 512];
        for _ in 0..200 {
            pool.render(&mut buf, &params(), SR);
        }
        assert_eq!(pool.active_count(), 1);
    }

    #[test]
    fn active_pitch_classes_stays_lit_while_a_different_note_of_the_same_class_still_sounds() {
        // Two keys an octave apart - e.g. the default keyboard mapping's A
        // (note 60, C4) and K (note 72, C5) - are two different voices that
        // happen to share a pitch class. Releasing one must not blank that
        // class while the other is still sounding. This holds structurally
        // here, for free: the mask is folded fresh from whichever voices
        // currently hold `Some(note)`, never from a per-class counter a
        // single `note_off` could decrement to zero regardless of who else
        // is still holding that class.
        let mut pool = VoicePool::default();
        pool.note_on(60, 1.0, SR);
        pool.note_on(72, 1.0, SR);
        assert_ne!(pool.active_pitch_classes() & 1, 0, "C should be lit");

        pool.note_off(60);
        let mut buf = vec![0.0f32; 512];
        // Long enough for note 60's voice to actually reach idle and free
        // itself - see `a_released_voice_eventually_becomes_idle_and_free`
        // for why 400 renders (not the 200 `releasing_a_note_frees_only_
        // that_voice` above uses for a coarser check) is the value with
        // headroom for a full release from any envelope stage.
        for _ in 0..400 {
            pool.render(&mut buf, &params(), SR);
        }
        assert!(
            pool.voices.iter().all(|v| v.note != Some(60)),
            "note 60's voice should have gone idle and freed by now"
        );
        assert_ne!(
            pool.active_pitch_classes() & 1,
            0,
            "C should still be lit - note 72 is still sounding"
        );

        pool.note_off(72);
        for _ in 0..400 {
            pool.render(&mut buf, &params(), SR);
        }
        assert_eq!(
            pool.active_pitch_classes(),
            0,
            "C should go dark once both voices are idle"
        );
    }

    #[test]
    fn retriggering_a_sounding_note_reuses_its_voice() {
        // Otherwise holding a key that repeats would consume the whole pool.
        let mut pool = VoicePool::default();
        pool.note_on(60, 1.0, SR);
        pool.note_on(60, 1.0, SR);
        assert_eq!(pool.active_count(), 1);
    }

    #[test]
    fn the_pool_steals_the_oldest_voice_when_full() {
        let mut pool = VoicePool::default();
        for i in 0..VOICE_COUNT {
            pool.note_on(40 + i as u8, 1.0, SR);
        }
        assert_eq!(pool.active_count(), VOICE_COUNT);
        pool.note_on(100, 1.0, SR);
        assert_eq!(
            pool.active_count(),
            VOICE_COUNT,
            "the pool grew past its limit"
        );
        // The first note played is the one that gave way.
        assert!(!pool.voices.iter().any(|v| v.note == Some(40)));
        assert!(pool.voices.iter().any(|v| v.note == Some(100)));
    }

    #[test]
    fn the_pool_steals_the_truly_oldest_voice_even_when_that_is_not_slot_zero() {
        // `the_pool_steals_the_oldest_voice_when_full` above fills every slot
        // exactly once, in order, so array position and age agree by
        // construction: slot 0 is both "first in the array" and "oldest by
        // age". A stand-in for stealing that just grabs `voices[0]` -
        // ignoring `age` entirely - passes that test for the wrong reason.
        // Confirmed: swapping `min_by_key(|v| v.age)` for
        // `.iter_mut().next()` leaves every one of the 129 tests in this
        // crate green.
        //
        // This test breaks that coincidence on purpose. Slot 0's original
        // voice is released and retriggered with a brand-new note, so slot 0
        // ends up holding the NEWEST age in the pool while the genuinely
        // oldest survivor sits in slot 1. Any implementation that steals by
        // array position rather than by age must now steal the wrong voice.
        let mut pool = VoicePool::default();
        for i in 0..VOICE_COUNT {
            pool.note_on(40 + i as u8, 1.0, SR);
        }
        // Slot 0 holds note 40 (age 1). No block has been rendered yet, so
        // every envelope is still sitting at level 0.0 in its initial Attack
        // stage - releasing from level 0.0 crosses the idle threshold on the
        // very first tick of the next render, so one block is enough to
        // free the slot.
        pool.note_off(40);
        let mut buf = vec![0.0f32; 512];
        pool.render(&mut buf, &params(), SR);
        assert!(
            pool.voices.iter().all(|v| v.note != Some(40)),
            "slot 0 never freed"
        );

        // Reuse the freed slot. It is the only free one, so the new note
        // lands back in slot 0 - but with the newest age in the pool. The
        // true oldest survivor (note 41, age 2) is now in slot 1.
        pool.note_on(200, 1.0, SR);
        assert_eq!(pool.active_count(), VOICE_COUNT);

        // One more note forces a steal. The correct victim is the genuinely
        // oldest survivor, note 41 - not slot 0's occupant (note 200, the
        // newest age in the pool).
        pool.note_on(201, 1.0, SR);
        assert_eq!(
            pool.active_count(),
            VOICE_COUNT,
            "the pool grew past its limit"
        );
        assert!(
            !pool.voices.iter().any(|v| v.note == Some(41)),
            "the genuinely oldest voice should have given way"
        );
        assert!(
            pool.voices.iter().any(|v| v.note == Some(200)),
            "the just-retriggered voice is the newest in the pool and must survive"
        );
        assert!(pool.voices.iter().any(|v| v.note == Some(201)));
    }

    #[test]
    fn note_off_for_a_note_that_is_not_sounding_is_harmless() {
        let mut pool = VoicePool::default();
        pool.note_off(60);
        assert_eq!(pool.active_count(), 0);
    }

    #[test]
    fn sixteen_simultaneous_voices_stay_in_range() {
        // Sixteen voices at full level would sum to sixteen. The pool must scale
        // so a chord does not clip before it reaches the mixer.
        let mut pool = VoicePool::default();
        for i in 0..VOICE_COUNT {
            pool.note_on(48 + i as u8, 1.0, SR);
        }
        let mut buf = vec![0.0f32; 512];
        for _ in 0..20 {
            buf.fill(0.0);
            pool.render(&mut buf, &params(), SR);
            for v in &buf {
                assert!(v.is_finite());
                assert!(v.abs() < 4.0, "sixteen voices summed to {v}");
            }
        }
    }

    #[test]
    fn loudness_scales_with_the_square_root_of_voice_count_not_a_constant() {
        // `sixteen_simultaneous_voices_stay_in_range` above always holds
        // exactly sixteen voices, so it only ever exercises `1.0 /
        // 16.0.sqrt()` - a single point on the curve. Replacing the whole
        // expression with the literal `0.25` (which equals `1.0 /
        // 16.0.sqrt()`) leaves every one of the 129 tests in this crate
        // green: nothing here checks that the scale factor actually moves
        // as the voice count changes.
        //
        // This test checks the relationship instead of one point on it: the
        // peak level of a chord of N freshly-started, identically-configured
        // voices, measured a few samples into the same attack ramp they all
        // share, relative to a single voice under the same conditions, must
        // land near sqrt(N) - not near N (no compensation at all) and not
        // flat at 1 (the pool ducking to a fixed level regardless of count).
        //
        // Measured within the first few samples deliberately: this is
        // before the different oscillator frequencies have drifted far
        // enough apart in phase to disturb the sum, and before the shared
        // filter's own transient has had time to diverge per voice, so the
        // *unscaled* sum of N voices is close to N times a single voice's
        // level - which is exactly the assumption sqrt(N) compensation is
        // supposed to correct for.
        fn peak_after(notes: &[u8], frames: usize) -> f32 {
            let mut pool = VoicePool::default();
            for &n in notes {
                pool.note_on(n, 1.0, SR);
            }
            let mut buf = vec![0.0f32; frames];
            pool.render(&mut buf, &params(), SR);
            buf.iter().fold(0.0f32, |a, b| a.max(b.abs()))
        }

        const FRAMES: usize = 8;
        let one = peak_after(&[60], FRAMES);
        let four = peak_after(&[60, 61, 62, 63], FRAMES);
        let sixteen = peak_after(
            &(0..VOICE_COUNT as u8).map(|i| 48 + i).collect::<Vec<_>>(),
            FRAMES,
        );
        assert!(
            one > 0.0,
            "the single voice produced no signal to compare against"
        );

        let ratio_four = four / one;
        let ratio_sixteen = sixteen / one;

        // sqrt(4) = 2, sqrt(16) = 4. A fixed scale (e.g. the always-0.25
        // mutation) applies the *same* divisor regardless of count, so the
        // raw N-times growth of the sum would show through unchecked and
        // these ratios would land near 4 and 16 instead - comfortably
        // outside the tolerance below in either direction.
        assert!(
            (ratio_four - 2.0).abs() < 0.5,
            "four voices were {ratio_four:.3}x one voice, expected close to sqrt(4) = 2.0"
        );
        assert!(
            (ratio_sixteen - 4.0).abs() < 1.0,
            "sixteen voices were {ratio_sixteen:.3}x one voice, expected close to sqrt(16) = 4.0"
        );
    }
}

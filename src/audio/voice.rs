use crate::audio::dsp::{Adsr, AdsrParams, Osc, Svf, Waveform};
use crate::core::music::note_to_freq;
use crate::params::registry::{self as p, ParamRegistry, PARAM_COUNT};
use crate::params::smoothing::Smoother;

pub const VOICE_COUNT: usize = 16;

/// Time constant for the polyphony scale glide, in milliseconds.
///
/// Chosen at the architecture's stated one-pole smoothing time (§4, "one-pole,
/// ~5 ms"), and the two ends of the range were checked rather than assumed.
///
/// Fast enough not to pump: a one-pole is ~95 % settled after three time
/// constants, so 5 ms is finished in 15 ms. Even a violent tremolo puts notes
/// 60 ms apart, so the scale is always at rest again before the next note
/// changes it - there is no accumulating lag to hear as pumping, and no chord
/// held with the level still visibly on its way somewhere.
///
/// Slow enough to remove the step: the largest jump this factor can make in
/// ordinary play is one note added to one already sounding, 1.0 to 0.7071.
/// Spread over a 5 ms one-pole at 48 kHz that is 0.0012 of gain in the first
/// sample instead of 0.2929 - a 240-fold reduction, and the transition's
/// energy above 4 kHz (where a click actually lives) is some 42 dB down on the
/// step's, because a one-pole's step response rolls off at 6 dB per octave
/// above 1/(2*pi*tau) = 32 Hz while a true step does not roll off at all.
const SCALE_GLIDE_MS: f32 = 5.0;

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
    velocity_gain: Smoother,
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
            velocity_gain: Smoother::new(48_000.0, 5.0),
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
        // Whether this voice is already sounding decides how the handover is
        // made. It is read before `note` is overwritten.
        let taking_over = self.note.is_some();
        self.note = Some(note);
        let velocity = velocity.clamp(0.0, 1.0);
        if taking_over {
            self.velocity_gain.set_target(velocity);
        } else {
            self.velocity_gain.snap(velocity);
        }
        self.velocity = velocity;
        // Oscillator phases are deliberately not reset. Restarting every voice
        // from phase zero makes stacked notes sum coherently on their first
        // cycle, which reads as a click at the start of a chord.
        if !taking_over {
            // A voice that has been idle still holds whatever its filter was
            // left ringing with, frozen since the note ended - `render`
            // returns early once `note` is None, so nothing decays it. A new
            // note starts from a defined state rather than from a residue of
            // whichever note happened to use this slot last.
            self.filter.reset();
        } else {
            // Taking a voice over from a note that is still sounding is a
            // different matter, and resetting here was the bug: the filter's
            // integrators carry the signal, so zeroing them dropped the
            // output to nothing in a single sample. Measured on the stolen
            // voice at a 300 Hz cutoff, the step was ten times the waveform's
            // own slope - an abrupt cut, which is exactly what a click is.
            //
            // Keeping the state hands the voice over instead. The filter's
            // stored energy decays with its own time constant while the new
            // note's oscillator establishes itself, so what is heard is the
            // oldest note changing pitch rather than being cut off. The
            // envelope already behaves the same way: `gate_off` and
            // `gate_on` both continue from the current level rather than
            // jumping.
        }
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
        for sample in out.iter_mut() {
            let env = self.env.tick(&adsr, sample_rate);
            let raw = 0.5
                * (self.osc_a.tick(freq_a, sample_rate, wave)
                    + self.osc_b.tick(freq_b, sample_rate, wave));
            let filtered = self.filter.lowpass(raw, cutoff, resonance, sample_rate);
            *sample += filtered * env * level * self.velocity_gain.next();
        }

        if self.env.is_idle() {
            self.note = None;
        }
    }
}

pub struct VoicePool {
    pub voices: [Voice; VOICE_COUNT],
    next_age: u64,
    /// The polyphony scale actually applied, glided rather than stepped.
    scale: Smoother,
    /// The sample rate `scale`'s coefficient was computed for, so a device
    /// change retunes it instead of silently smoothing over the wrong span.
    scale_rate: f32,
    /// Whether the previous block produced any voice output. A scale change
    /// is only discontinuous if there was already something for it to be
    /// discontinuous against.
    sounded: bool,
}

impl Default for VoicePool {
    fn default() -> Self {
        let mut scale = Smoother::new(48_000.0, SCALE_GLIDE_MS);
        // The scale for a silent pool is 1.0, not 0.0: starting at zero would
        // fade the first note in over 5 ms it did not ask for.
        scale.snap(1.0);
        VoicePool {
            voices: std::array::from_fn(|_| Voice::default()),
            next_age: 0,
            scale,
            scale_rate: 48_000.0,
            sounded: false,
        }
    }
}

impl VoicePool {
    pub fn panic(&mut self) {
        for voice in &mut self.voices {
            *voice = Voice::default();
        }
        self.scale.snap(1.0);
        self.sounded = false;
    }

    pub fn all_notes_off(&mut self) {
        for voice in &mut self.voices {
            if voice.note.is_some() {
                voice.release();
            }
        }
    }

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

        // Steal the oldest. The voice is handed over rather than cut: it
        // keeps its filter state and its envelope level, so the note it was
        // playing changes pitch instead of stopping dead (see `Voice::start`).
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
        // note.
        //
        // That factor changes whenever the voice count does, and applying the
        // new one to a whole block is a step: with one note sustaining, adding
        // a second multiplied the buffer by 1/sqrt(2) between the last sample
        // of one block and the first of the next. Measured, the arriving voice
        // moved the signal by 29.29 % of it in that one sample - at every
        // block size, and before its own envelope had produced anything at
        // all. That is a click, not a duck. Playing a chord one finger at a
        // time is the first thing anyone does with a synth, so the factor is
        // glided to its new value one sample at a time instead; the same
        // measurement after the glide reads 0.12 %.
        //
        // An earlier version of this comment called that step "twelve times
        // the waveform's own slope". It is not. The denominator used there was
        // the largest movement in the sixteen samples after the boundary,
        // while this file's tests measure slope across the whole block; on
        // that yardstick the step is 1.04x the waveform's own movement -
        // still a real discontinuity, but not the order of magnitude claimed.
        // The 29.29 % above needs no yardstick: it is exactly 1 - 1/sqrt(2),
        // and unlike a slope ratio it does not depend on where in the
        // waveform's cycle the block boundary happens to fall.
        //
        // The cost, measured: if the voice count jumps a long way inside a
        // single block, the scale is briefly too high for the number of
        // voices now sounding. Fifteen notes landing on one already held peaks
        // at 1.39 to 2.13 across twenty-four readings - four block sizes by
        // six positions in the held note's envelope - against 1.0989 for the
        // same chord struck from silence. So a 1.3x to 1.9x overshoot into the
        // master clipper, lasting the 15 ms the glide needs to settle. The
        // span is the honest figure rather than any single pair out of it,
        // because which reading you get depends on where in the envelope the
        // chord lands; an earlier version of this comment quoted one near the
        // top of the span as though it were the number.
        //
        // It is bounded - the clipper is the bound - it is a gesture no pair
        // of hands can make on a thirteen-key layout, and the alternative is a
        // click every time a note is added. Adding one note to one held note,
        // the case this exists for, overshoots by nothing worth measuring.
        let active = self.active_count();
        let target = 1.0 / (active.max(1) as f32).sqrt();

        if self.scale_rate != sample_rate {
            self.scale_rate = sample_rate;
            self.scale.set_time(sample_rate, SCALE_GLIDE_MS);
        }

        if self.sounded {
            self.scale.set_target(target);
            for s in out.iter_mut() {
                *s *= self.scale.next();
            }
        } else {
            // Nothing was sounding last block, so there is no signal for the
            // change to be discontinuous against - and gliding here would be
            // actively worse than useless. A four-note chord struck from
            // silence would start at the one-voice scale and duck into place,
            // pushing roughly four times the intended level into the master
            // clipper on the way. Snapping is both inaudible and safer.
            self.scale.snap(target);
            for s in out.iter_mut() {
                *s *= target;
            }
        }

        self.sounded = active > 0;
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
        // This test is also what pins the "snap from silence" half of the
        // scale glide (see `VoicePool::render`): `peak_after` builds a fresh
        // pool for every measurement, so nothing was sounding beforehand and
        // the scale must already be at 1/sqrt(N) on the very first sample.
        // Make the glide unconditional and `ratio_four` reads 4.0 instead of
        // 2.0 - the chord starts at the one-voice scale and ducks into place,
        // which is both audible and four times too loud into the clipper.
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
    #[test]
    fn adding_a_note_to_a_held_one_does_not_step_the_signal() {
        // Playing a chord one finger at a time is the first thing anyone does
        // with a synth, and the polyphony scale used to make it click: with
        // note 60 sustaining, the arrival of a second voice multiplied the
        // whole buffer by 1/sqrt(2) between the last sample of one block and
        // the first of the next.
        //
        // The artefact is isolated by running the identical scenario twice -
        // once pressing a second key at the block boundary, once not - and
        // comparing the first sample of the block that follows. The second
        // voice's own envelope is still at zero on that sample (attack
        // defaults to 5 ms = 240 samples), so any difference between the two
        // runs is the scale factor moving, and nothing else. That makes the
        // measurement independent of where in the waveform's cycle the
        // boundary happens to fall, which a bare sample-to-sample delta is
        // not.
        //
        // Measured: without the glide the difference is 29.29 % of the sample
        // - exactly 1 - 1/sqrt(2) - at every block size. With it, 0.12 %.
        fn first_sample_after_boundary(block: usize, press_second_key: bool) -> f32 {
            let mut pool = VoicePool::default();
            let mut buf = vec![0.0f32; block];
            pool.note_on(60, 1.0, SR);
            for _ in 0..40 {
                buf.fill(0.0);
                pool.render(&mut buf, &params(), SR);
            }
            if press_second_key {
                pool.note_on(64, 1.0, SR);
            }
            buf.fill(0.0);
            pool.render(&mut buf, &params(), SR);
            buf[0]
        }

        for block in [64usize, 128, 256, 512] {
            let alone = first_sample_after_boundary(block, false);
            let chord = first_sample_after_boundary(block, true);
            assert!(
                alone.abs() > 1e-3,
                "block {block}: nothing was sounding to be discontinuous, \
                 the measurement proves nothing"
            );
            let artefact = (chord - alone).abs() / alone.abs();
            assert!(
                artefact < 0.01,
                "block {block}: pressing a second key moved the signal by \
                 {:.2} % in one sample ({alone:+.5} -> {chord:+.5}); an \
                 unglided scale factor gives 29.29 %",
                artefact * 100.0
            );
        }
    }

    #[test]
    fn the_scale_glide_settles_rather_than_lagging_behind_fast_playing() {
        // The other half of the time-constant choice. A glide still moving
        // when the next note lands turns every fast passage into audible
        // pumping, so the factor has to be at rest again long before a person
        // can play the next note. A one-pole is ~95 % settled after three time
        // constants, which for 5 ms is 15 ms.
        //
        // The factor is read out of the pool rather than assumed: two pools
        // play note 60 identically, and one of them additionally starts a
        // voice at velocity 0. That voice contributes exactly nothing to the
        // sum (`amp = level * velocity`) but does raise the voice count, so
        // the ratio between the two pools' output IS the scale factor, sample
        // for sample, with no other difference to confuse it.
        fn scale_after(frames: usize) -> f32 {
            let mut alone = VoicePool::default();
            let mut with_extra = VoicePool::default();
            let mut a = vec![0.0f32; 512];
            let mut b = vec![0.0f32; 512];
            for pool in [&mut alone, &mut with_extra] {
                pool.note_on(60, 1.0, SR);
            }
            for _ in 0..40 {
                a.fill(0.0);
                b.fill(0.0);
                alone.render(&mut a, &params(), SR);
                with_extra.render(&mut b, &params(), SR);
            }
            with_extra.note_on(64, 0.0, SR);

            let mut a = vec![0.0f32; frames];
            let mut b = vec![0.0f32; frames];
            alone.render(&mut a, &params(), SR);
            with_extra.render(&mut b, &params(), SR);
            let last_a = *a.last().unwrap();
            assert!(
                last_a.abs() > 1e-3,
                "the reference voice was silent, the ratio means nothing"
            );
            b.last().unwrap() / last_a
        }

        let target = 1.0 / 2.0f32.sqrt();
        // One sample in, the factor has barely moved - that is the whole point
        // of the glide, and what makes the added note inaudible as a step.
        let immediate = scale_after(1);
        assert!(
            immediate > 0.99,
            "the scale jumped to {immediate:.4} within one sample of the \
             second voice arriving; that is the step this glide exists to \
             remove"
        );
        // Fifteen milliseconds in, it has arrived.
        let settled = scale_after(720);
        let remaining = (settled - target).abs() / (1.0 - target);
        assert!(
            remaining < 0.05,
            "after 15 ms the scale was {settled:.4}, still {:.1} % short of \
             its target {target:.4} - a glide that slow pumps on fast playing",
            remaining * 100.0
        );
    }
    /// What the voice's filter has stored, observed through the filter's own
    /// behaviour: feeding it silence returns its natural response, which is
    /// zero exactly when its integrators are.
    ///
    /// This probe advances the filter by one sample, so it is only ever used
    /// at the end of what a test is asserting about.
    fn filter_residue(v: &mut Voice, cutoff: f32, q: f32) -> f32 {
        v.filter.lowpass(0.0, cutoff, q, SR).abs()
    }

    /// Params with the filter somewhere dark, where the theft was worst.
    fn dark_params(cutoff: f32, q: f32) -> ParamValues {
        let reg = crate::params::registry::ParamRegistry::new();
        let mut pv = params();
        pv.0[p::FILTER_CUTOFF.0 as usize] = reg.normalize(p::FILTER_CUTOFF, cutoff);
        pv.0[p::FILTER_RESONANCE.0 as usize] = reg.normalize(p::FILTER_RESONANCE, q);
        pv
    }

    #[test]
    fn stealing_a_voice_hands_it_over_instead_of_cutting_it() {
        // Sixteen notes held and a seventeenth played is ordinary: thirteen
        // keys plus release tails gets there inside half a second. The theft
        // used to reset the stolen voice's filter, and since the filter's
        // integrators carry the signal, the output dropped to nothing in a
        // single sample.
        //
        // Measured on the stolen voice itself, with no other voices to hide
        // behind, as a multiple of the waveform's own sample-to-sample
        // movement - the same yardstick the polyphony-scale glide uses:
        //
        //   cutoff 2 kHz            2.74x before   0.06x after
        //   cutoff 300 Hz          10.22x before   0.92x after
        //   cutoff 400 Hz, Q 18     6.99x before   0.75x after
        //   cutoff 12 kHz           0.45x before   0.01x after
        for (cutoff, q) in [(2_000.0f32, 1.0f32), (300.0, 1.0), (400.0, 18.0)] {
            let pv = dark_params(cutoff, q);
            let mut v = Voice::default();
            v.start(48, 1.0, SR);
            let mut buf = vec![0.0f32; 512];
            for _ in 0..40 {
                buf.fill(0.0);
                v.render(&mut buf, &pv, SR);
            }
            let last = *buf.last().unwrap();
            let own_slope = buf
                .windows(2)
                .fold(0.0f32, |a, w| a.max((w[1] - w[0]).abs()));
            assert!(own_slope > 0.0, "the voice was silent, nothing to steal");

            v.start(80, 1.0, SR);
            buf.fill(0.0);
            v.render(&mut buf, &pv, SR);
            let step = (buf[0] - last).abs();
            assert!(
                step <= own_slope,
                "at cutoff {cutoff} Hz / Q {q} the theft moved the signal \
                 {step:.5} in one sample, more than the {own_slope:.5} the \
                 waveform moves on its own - that is a cut, not a handover"
            );

            // And the mechanism, so a future change cannot pass the
            // measurement above by accident: the voice kept its filter.
            assert!(
                filter_residue(&mut v, cutoff, q) > 1e-6,
                "the stolen voice's filter was emptied"
            );
        }
    }

    #[test]
    fn a_voice_reused_after_going_idle_starts_from_a_clean_filter() {
        // The other half of the same decision. A voice that has gone idle is
        // not sounding, so there is nothing to hand over - and its filter has
        // been frozen since the note ended, because `render` returns early
        // once `note` is None and nothing decays it. A new note must not
        // begin inside the residue of whichever note used this slot last.
        let (cutoff, q) = (400.0f32, 18.0f32);
        let pv = dark_params(cutoff, q);
        let mut v = Voice::default();
        v.start(48, 1.0, SR);
        let mut buf = vec![0.0f32; 512];
        for _ in 0..40 {
            buf.fill(0.0);
            v.render(&mut buf, &pv, SR);
        }
        v.release();
        for _ in 0..600 {
            buf.fill(0.0);
            v.render(&mut buf, &pv, SR);
        }
        assert!(v.is_idle(), "the voice never freed itself");
        assert!(
            filter_residue(&mut v, cutoff, q) > 1e-6,
            "the filter emptied itself on the way to idle, so this test \
             cannot tell whether starting a note clears it"
        );

        v.start(48, 1.0, SR);
        assert_eq!(
            filter_residue(&mut v, cutoff, q),
            0.0,
            "a reused voice began inside the previous note's filter"
        );
    }
}

macro_rules! id_type {
    ($name:ident, $inner:ty) => {
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        pub struct $name(pub $inner);
    };
}

id_type!(ParamId, u16);
id_type!(ModuleId, u16);
id_type!(TrackId, u8);
id_type!(MacroId, u8);
id_type!(ModSourceId, u16);
id_type!(DeviceId, u16);
id_type!(MidiPortId, u16);

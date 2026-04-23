mod info;
mod mp4;
mod trimmer;

pub use info::GaplessInfo;
pub use mp4::probe_mp4_gapless;
pub(crate) use mp4::probe_mp4_gapless_dyn;
pub use trimmer::GaplessTrimmer;

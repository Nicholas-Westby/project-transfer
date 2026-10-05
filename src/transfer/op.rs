//! The small changes besides file content, applied the same way by the
//! computer that pulls and by the one that receives a push.

use super::Applier;

#[derive(Clone, Copy)]
pub enum Op<'a> {
    MakeDir,
    Symlink(&'a str),
    SetMtime(i64),
    Remove(bool),
}

impl Op<'_> {
    /// Applies it under `a`. A failure says which step failed, since the
    /// same disk error means different things for each.
    pub fn apply(self, a: &Applier, rel: &str) -> Result<(), String> {
        let (step, done) = match self {
            Op::MakeDir => ("create the folder", a.make_dir(rel)),
            Op::Symlink(target) => ("create the link", a.make_symlink(rel, target)),
            Op::SetMtime(ms) => ("set its modified time", a.set_mtime(rel, ms)),
            Op::Remove(is_dir) => ("remove it", a.remove(rel, is_dir)),
        };
        done.map_err(|e| format!("Could not {step}: {e}"))
    }
}

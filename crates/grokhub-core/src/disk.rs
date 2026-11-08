//! One disk's space, as the system readers report it and Home shows it.

/// Free and total bytes of one disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskUse {
    /// `/`, `home`, or a Windows drive such as `C:`.
    pub label: String,
    pub total: u64,
    pub free: u64,
}

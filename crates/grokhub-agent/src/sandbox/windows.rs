//! Windows: a write-restricted token (the same idea as Chromium's and Codex's
//! Windows sandboxes). Reads and execs are unchanged; a write must be allowed
//! both to the user and to one of the restricting SIDs: Loadout's own
//! sandbox SID, Everyone (devices such as NUL) and the logon SID (the
//! window station). The writable roots get one inheritable ACE for the
//! sandbox SID, added once and kept; a normal token never checks it.
//!
//! The process still runs in a Job object (see `tools::shell::win`), so a
//! stop kills the whole tree.

use std::collections::HashSet;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::{Mutex, OnceLock};

use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, GENERIC_ALL, HANDLE};
use windows_sys::Win32::Security::Authorization::{
    GetNamedSecurityInfoW, SetEntriesInAclW, SetNamedSecurityInfoW, EXPLICIT_ACCESS_W,
    GRANT_ACCESS, NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT, TRUSTEE_IS_SID, TRUSTEE_IS_UNKNOWN,
    TRUSTEE_W,
};
use windows_sys::Win32::Security::{
    AclSizeInformation, AddAccessAllowedAceEx, CopySid, CreateRestrictedToken, CreateWellKnownSid,
    EqualSid, GetAce, GetAclInformation, GetLengthSid, GetTokenInformation, InitializeAcl,
    SetTokenInformation, TokenDefaultDacl, TokenGroups, TokenUser, WinLocalSystemSid, WinWorldSid,
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_REVISION, ACL_SIZE_INFORMATION,
    DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_MAX_SID_SIZE,
    SID_AND_ATTRIBUTES, SUB_CONTAINERS_AND_OBJECTS_INHERIT, TOKEN_ADJUST_DEFAULT,
    TOKEN_ASSIGN_PRIMARY, TOKEN_DEFAULT_DACL, TOKEN_DUPLICATE, TOKEN_GROUPS, TOKEN_QUERY,
    TOKEN_USER, WRITE_RESTRICTED,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

const SE_GROUP_LOGON_ID: u32 = 0xC000_0000;
const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
/// What a writable root grants the sandbox SID.
const ROOT_RIGHTS: u32 = FILE_GENERIC_READ | FILE_GENERIC_WRITE | FILE_GENERIC_EXECUTE | DELETE;

/// A SID in owned memory.
struct Sid(Vec<u8>);

impl Sid {
    /// SAFETY: `p` must point at a valid SID.
    unsafe fn copy(p: PSID) -> Result<Self, String> {
        // SAFETY: the caller passes a valid SID.
        let len = unsafe { GetLengthSid(p) };
        let mut buf = vec![0u8; len as usize];
        // SAFETY: buf has the SID's length.
        if unsafe { CopySid(len, buf.as_mut_ptr().cast(), p) } == 0 {
            return Err(last_error("CopySid"));
        }
        Ok(Self(buf))
    }

    fn psid(&self) -> PSID {
        self.0.as_ptr().cast_mut().cast()
    }
}

fn last_error(what: &str) -> String {
    format!("{what}: {}", std::io::Error::last_os_error())
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain([0]).collect()
}

/// The sandbox SID's sub-authorities ("LOAD" "OUTS" "ANDB" "OX" 1), under
/// the resource-manager authority (S-1-9), which is set aside for SIDs an
/// application defines. With a capability SID (S-1-15-3) among the
/// restricting SIDs, `CreateRestrictedToken` failed with error 87.
const SANDBOX_SUBAUTHORITIES: [u32; 4] = [0x4C4F_4144, 0x4F55_5453, 0x414E_4442, 0x4F58_0001];

/// The bytes of a SID: revision 1, the sub-authority count, a 48-bit
/// big-endian authority, then each sub-authority little-endian.
fn sid_bytes(authority: u8, subs: &[u32]) -> Vec<u8> {
    let mut out = vec![1, subs.len() as u8, 0, 0, 0, 0, 0, authority];
    for sub in subs {
        out.extend_from_slice(&sub.to_le_bytes());
    }
    out
}

/// Loadout's sandbox SID (the same on every run and machine). Writable roots
/// grant it write access; the restricted token lists it as a restricting SID.
fn sandbox_sid() -> &'static Sid {
    static SID: OnceLock<Sid> = OnceLock::new();
    SID.get_or_init(|| Sid(sid_bytes(9, &SANDBOX_SUBAUTHORITIES)))
}

fn well_known(kind: i32) -> Result<Sid, String> {
    let mut buf = vec![0u8; SECURITY_MAX_SID_SIZE as usize];
    let mut size = SECURITY_MAX_SID_SIZE;
    // SAFETY: buf holds SECURITY_MAX_SID_SIZE bytes.
    if unsafe { CreateWellKnownSid(kind, ptr::null_mut(), buf.as_mut_ptr().cast(), &mut size) } == 0
    {
        return Err(last_error("CreateWellKnownSid"));
    }
    buf.truncate(size as usize);
    Ok(Sid(buf))
}

/// A token's information block, read into an aligned buffer.
fn token_info(token: HANDLE, class: i32) -> Result<Vec<u64>, String> {
    let mut len = 0u32;
    // SAFETY: a size query with a null buffer.
    unsafe { GetTokenInformation(token, class, ptr::null_mut(), 0, &mut len) };
    if len == 0 {
        return Err(last_error("GetTokenInformation"));
    }
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    // SAFETY: buf holds at least `len` bytes, 8-aligned.
    if unsafe { GetTokenInformation(token, class, buf.as_mut_ptr().cast(), len, &mut len) } == 0 {
        return Err(last_error("GetTokenInformation"));
    }
    Ok(buf)
}

/// A restricted primary token for `CreateProcessAsUserW`.
pub struct Restricted(HANDLE);

impl Restricted {
    pub fn handle(&self) -> HANDLE {
        self.0
    }
}

impl Drop for Restricted {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateRestrictedToken and is closed once.
        unsafe { CloseHandle(self.0) };
    }
}

// SAFETY: a token handle may be used and closed from any thread.
unsafe impl Send for Restricted {}

/// The write-restricted copy of this process's token.
pub fn restricted_token() -> Result<Restricted, String> {
    let cap = sandbox_sid();
    let world = well_known(WinWorldSid)?;
    let system = well_known(WinLocalSystemSid)?;
    let mut own: HANDLE = ptr::null_mut();
    // SAFETY: the pseudo handle needs no closing; `own` is closed below.
    let opened = unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT,
            &mut own,
        )
    };
    if opened == 0 {
        return Err(last_error("OpenProcessToken"));
    }
    let result = build(own, cap, &world, &system);
    // SAFETY: `own` came from OpenProcessToken.
    unsafe { CloseHandle(own) };
    result
}

/// For tests: try a write-restricted token with each restricting SID on its
/// own, so a failure names the SID it comes from.
#[cfg(test)]
pub(crate) fn probe_restricting_sids() -> String {
    let mut own: HANDLE = ptr::null_mut();
    // SAFETY: the pseudo handle needs no closing; `own` is closed below.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_DUPLICATE | TOKEN_QUERY, &mut own) }
        == 0
    {
        return last_error("OpenProcessToken");
    }
    let mut lines = Vec::new();
    let mut candidates: Vec<(&str, Sid)> =
        vec![("sandbox", Sid(sid_bytes(9, &SANDBOX_SUBAUTHORITIES)))];
    if let Ok(world) = well_known(WinWorldSid) {
        candidates.push(("everyone", world));
    }
    for (name, sid) in &candidates {
        let one = [SID_AND_ATTRIBUTES {
            Sid: sid.psid(),
            Attributes: 0,
        }];
        let mut token: HANDLE = ptr::null_mut();
        // SAFETY: the SID array lives until the call returns.
        let made = unsafe {
            CreateRestrictedToken(
                own,
                WRITE_RESTRICTED,
                0,
                ptr::null(),
                0,
                ptr::null(),
                1,
                one.as_ptr(),
                &mut token,
            )
        };
        if made == 0 {
            lines.push(format!("{name}: {}", last_error("CreateRestrictedToken")));
        } else {
            lines.push(format!("{name}: ok"));
            // SAFETY: the handle came from CreateRestrictedToken.
            unsafe { CloseHandle(token) };
        }
    }
    // SAFETY: `own` came from OpenProcessToken.
    unsafe { CloseHandle(own) };
    lines.join("; ")
}

fn build(own: HANDLE, cap: &Sid, world: &Sid, system: &Sid) -> Result<Restricted, String> {
    let user_buf = token_info(own, TokenUser)?;
    // SAFETY: the buffer holds a TOKEN_USER whose SID points inside it.
    let user = unsafe { Sid::copy((*user_buf.as_ptr().cast::<TOKEN_USER>()).User.Sid)? };
    let groups_buf = token_info(own, TokenGroups)?;
    // SAFETY: the buffer holds a TOKEN_GROUPS with GroupCount entries.
    let logon = unsafe {
        let groups = groups_buf.as_ptr().cast::<TOKEN_GROUPS>();
        let first = (*groups).Groups.as_ptr();
        (0..(*groups).GroupCount as usize)
            .map(|i| &*first.add(i))
            .find(|g| g.Attributes & SE_GROUP_LOGON_ID == SE_GROUP_LOGON_ID)
            .map(|g| Sid::copy(g.Sid))
            .transpose()?
    };
    let mut restrict = vec![
        SID_AND_ATTRIBUTES {
            Sid: cap.psid(),
            Attributes: 0,
        },
        SID_AND_ATTRIBUTES {
            Sid: world.psid(),
            Attributes: 0,
        },
    ];
    if let Some(logon) = &logon {
        restrict.push(SID_AND_ATTRIBUTES {
            Sid: logon.psid(),
            Attributes: 0,
        });
    }
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: the SID array lives until the call returns.
    let made = unsafe {
        CreateRestrictedToken(
            own,
            WRITE_RESTRICTED,
            0,
            ptr::null(),
            0,
            ptr::null(),
            restrict.len() as u32,
            restrict.as_ptr(),
            &mut token,
        )
    };
    if made == 0 {
        return Err(last_error("CreateRestrictedToken"));
    }
    let token = Restricted(token);
    // Objects the child makes without a parent folder (pipes, events) get the
    // default DACL; it must name a restricting SID or the child can't write them.
    let mut owners: Vec<&Sid> = vec![&user, system, cap];
    if let Some(logon) = &logon {
        owners.push(logon);
    }
    let mut acl = default_dacl(&owners)?;
    let dacl = TOKEN_DEFAULT_DACL {
        DefaultDacl: acl.as_mut_ptr().cast::<ACL>(),
    };
    // SAFETY: dacl points at a valid ACL that lives until the call returns.
    let set = unsafe {
        SetTokenInformation(
            token.handle(),
            TokenDefaultDacl,
            (&dacl as *const TOKEN_DEFAULT_DACL).cast(),
            std::mem::size_of::<TOKEN_DEFAULT_DACL>() as u32,
        )
    };
    if set == 0 {
        return Err(last_error("SetTokenInformation"));
    }
    Ok(token)
}

fn default_dacl(sids: &[&Sid]) -> Result<Vec<u64>, String> {
    let ace = std::mem::size_of::<ACCESS_ALLOWED_ACE>() - std::mem::size_of::<u32>();
    let size = std::mem::size_of::<ACL>() + sids.iter().map(|s| ace + s.0.len()).sum::<usize>();
    let size = size.next_multiple_of(4);
    let mut buf = vec![0u64; size.div_ceil(8)];
    let acl = buf.as_mut_ptr().cast::<ACL>();
    // SAFETY: buf holds `size` bytes, enough for the header and every ACE.
    unsafe {
        if InitializeAcl(acl, size as u32, ACL_REVISION) == 0 {
            return Err(last_error("InitializeAcl"));
        }
        for sid in sids {
            if AddAccessAllowedAceEx(acl, ACL_REVISION, 0, GENERIC_ALL, sid.psid()) == 0 {
                return Err(last_error("AddAccessAllowedAceEx"));
            }
        }
    }
    Ok(buf)
}

/// Give the sandbox SID write access beneath `root` (inherited by
/// everything in it), once. Already granted, directly or from a parent: no
/// change, so a big tree is walked only the first time.
pub fn grant(root: &Path) -> Result<(), String> {
    static DONE: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    let done = DONE.get_or_init(Default::default);
    if done
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(root)
    {
        return Ok(());
    }
    let cap = sandbox_sid();
    let name = wide(root);
    let mut dacl: *mut ACL = ptr::null_mut();
    let mut sd: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: out-pointers are valid; `sd` is freed with LocalFree below.
    let rc = unsafe {
        GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut sd,
        )
    };
    if rc != 0 {
        return Err(format!("GetNamedSecurityInfoW: error {rc}"));
    }
    // SAFETY: dacl belongs to sd, which stays alive until freed below.
    let result = unsafe {
        if !dacl.is_null() && has_grant(dacl, cap) {
            Ok(())
        } else {
            add_grant(&name, dacl, cap)
        }
    };
    // SAFETY: sd came from GetNamedSecurityInfoW.
    unsafe { LocalFree(sd) };
    if result.is_ok() {
        done.lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(root.to_path_buf());
    }
    result
}

/// SAFETY: `dacl` must be a valid ACL.
unsafe fn has_grant(dacl: *const ACL, cap: &Sid) -> bool {
    let mut info = ACL_SIZE_INFORMATION::default();
    // SAFETY: info is the size the class asks for.
    let ok = unsafe {
        GetAclInformation(
            dacl,
            (&mut info as *mut ACL_SIZE_INFORMATION).cast(),
            std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    };
    if ok == 0 {
        return false;
    }
    (0..info.AceCount).any(|i| {
        let mut ace: *mut core::ffi::c_void = ptr::null_mut();
        // SAFETY: i is below AceCount; an allowed ACE's SID starts at SidStart.
        unsafe {
            if GetAce(dacl, i, &mut ace) == 0 {
                return false;
            }
            if (*ace.cast::<ACE_HEADER>()).AceType != ACCESS_ALLOWED_ACE_TYPE {
                return false;
            }
            let allowed = ace.cast::<ACCESS_ALLOWED_ACE>();
            let sid: PSID = (&raw mut (*allowed).SidStart).cast();
            EqualSid(sid, cap.psid()) != 0 && (*allowed).Mask & ROOT_RIGHTS == ROOT_RIGHTS
        }
    })
}

/// SAFETY: `name` is a NUL-terminated path; `old` is null or a valid ACL.
unsafe fn add_grant(name: &[u16], old: *const ACL, cap: &Sid) -> Result<(), String> {
    let entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: ROOT_RIGHTS,
        grfAccessMode: GRANT_ACCESS,
        grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: ptr::null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_UNKNOWN,
            ptstrName: cap.psid().cast(),
        },
    };
    let mut new_acl: *mut ACL = ptr::null_mut();
    // SAFETY: one valid entry; new_acl is LocalAlloc'd and freed below.
    let rc = unsafe { SetEntriesInAclW(1, &entry, old, &mut new_acl) };
    if rc != 0 {
        return Err(format!("SetEntriesInAclW: error {rc}"));
    }
    // SAFETY: name is NUL-terminated; new_acl is valid.
    let rc = unsafe {
        SetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            new_acl,
            ptr::null(),
        )
    };
    // SAFETY: new_acl came from SetEntriesInAclW.
    unsafe { LocalFree(new_acl.cast()) };
    if rc != 0 {
        return Err(format!("SetNamedSecurityInfoW: error {rc}"));
    }
    Ok(())
}

/// The token and environment for one sandboxed command. Without the
/// workspace grant there is no token (writes run as before), since a token
/// that can't write the workspace would stop all work.
pub struct Prepared {
    pub token: Option<Restricted>,
    pub env: Vec<(String, String)>,
}

pub fn prepare(plan: &super::Plan) -> Prepared {
    let env = proxy_env(plan);
    let token = match plan.writable.first().map(|ws| grant(ws)) {
        Some(Ok(())) => {
            for root in &plan.writable[1..] {
                let _ = grant(root);
            }
            restricted_token().ok()
        }
        _ => None,
    };
    Prepared { token, env }
}

/// The proxy variables for one command (and the ones it must not inherit).
pub fn proxy_env(plan: &super::Plan) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = Vec::new();
    if let Some(addr) = &plan.proxy {
        for (key, value) in super::proxy::env(addr) {
            // Windows names are case-insensitive: one of each.
            if !env.iter().any(|(k, _)| k.eq_ignore_ascii_case(key)) {
                env.push((key.to_string(), value));
            }
        }
        for key in super::proxy::UNSET {
            if !env.iter().any(|(k, _)| k.eq_ignore_ascii_case(key)) {
                env.push(((*key).to_string(), String::new()));
            }
        }
    }
    env
}

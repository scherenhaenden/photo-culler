//! Catalog snapshots. The caller must suspend changes to sidecars until `create`
//! returns; SQLite itself is copied through its online backup API (including WAL).
//! Snapshots and restores require new, empty destination paths.
use std::{
    ffi::{c_char, c_int, c_void, CStr, CString},
    fs,
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    ptr,
};

#[link(name = "sqlite3")]
unsafe extern "C" {
    fn sqlite3_open_v2(
        name: *const c_char,
        db: *mut *mut c_void,
        flags: c_int,
        vfs: *const c_char,
    ) -> c_int;
    fn sqlite3_close(db: *mut c_void) -> c_int;
    fn sqlite3_backup_init(
        dst: *mut c_void,
        dn: *const c_char,
        src: *mut c_void,
        sn: *const c_char,
    ) -> *mut c_void;
    fn sqlite3_backup_step(backup: *mut c_void, pages: c_int) -> c_int;
    fn sqlite3_backup_finish(backup: *mut c_void) -> c_int;
    fn sqlite3_exec(
        db: *mut c_void,
        sql: *const c_char,
        cb: Option<
            unsafe extern "C" fn(*mut c_void, c_int, *mut *mut c_char, *mut *mut c_char) -> c_int,
        >,
        arg: *mut c_void,
        err: *mut *mut c_char,
    ) -> c_int;
}
fn fail(s: &str) -> io::Error {
    io::Error::other(s.to_owned())
}
struct Db(*mut c_void);
impl Drop for Db {
    fn drop(&mut self) {
        unsafe {
            sqlite3_close(self.0);
        }
    }
}
fn open(path: &Path, flags: c_int) -> io::Result<Db> {
    use std::os::unix::ffi::OsStrExt;
    let path =
        CString::new(path.as_os_str().as_bytes()).map_err(|_| fail("invalid database path"))?;
    let mut db = ptr::null_mut();
    let rc = unsafe { sqlite3_open_v2(path.as_ptr(), &mut db, flags, ptr::null()) };
    let db = Db(db);
    if rc != 0 {
        return Err(fail("cannot open SQLite database"));
    }
    Ok(db)
}
fn integrity(db: &Db) -> io::Result<()> {
    unsafe extern "C" fn row(
        arg: *mut c_void,
        n: c_int,
        vals: *mut *mut c_char,
        _: *mut *mut c_char,
    ) -> c_int {
        if n == 1 && !(*vals).is_null() && CStr::from_ptr(*vals).to_bytes() == b"ok" {
            *(arg as *mut bool) = true;
        }
        0
    }
    let mut ok = false;
    let rc = unsafe {
        sqlite3_exec(
            db.0,
            c"PRAGMA integrity_check".as_ptr(),
            Some(row),
            &mut ok as *mut bool as *mut c_void,
            ptr::null_mut(),
        )
    };
    if rc != 0 || !ok {
        return Err(fail("SQLite integrity check failed"));
    }
    Ok(())
}
fn backup(src: &Path, dst: &Path) -> io::Result<()> {
    let source = open(src, 1)?;
    let target = open(dst, 2 | 4)?;
    let b = unsafe { sqlite3_backup_init(target.0, c"main".as_ptr(), source.0, c"main".as_ptr()) };
    if b.is_null() {
        return Err(fail("SQLite backup init failed"));
    }
    let rc = unsafe { sqlite3_backup_step(b, -1) };
    let finish = unsafe { sqlite3_backup_finish(b) };
    if rc != 101 || finish != 0 {
        return Err(fail("SQLite backup failed or source busy"));
    }
    // Make the standalone snapshot readable without WAL/SHM sidecars.
    if unsafe {
        sqlite3_exec(
            target.0,
            c"PRAGMA journal_mode=DELETE".as_ptr(),
            None,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    } != 0
    {
        return Err(fail("cannot finalize standalone snapshot"));
    }
    integrity(&target)
}
fn safe_relative(p: &Path) -> bool {
    !p.as_os_str().is_empty() && p.components().all(|c| matches!(c, Component::Normal(_)))
}
fn hash(path: &Path) -> io::Result<(u64, u64)> {
    let mut f = fs::File::open(path)?;
    let mut h = 14695981039346656037u64;
    let mut len = 0;
    let mut buf = [0; 16384];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        len += n as u64;
        for b in &buf[..n] {
            h = (h ^ u64::from(*b)).wrapping_mul(1099511628211);
        }
    }
    Ok((len, h))
}
fn collect(root: &Path, rel: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    let mut entries = fs::read_dir(root.join(rel))?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let r = rel.join(e.file_name());
        let t = e.file_type()?;
        if t.is_symlink() {
            return Err(fail("sidecar symlinks are unsupported"));
        }
        if t.is_dir() {
            collect(root, &r, out)?;
        } else if t.is_file() {
            out.push(r);
        } else {
            return Err(fail("unsupported sidecar type"));
        }
    }
    Ok(())
}
/// `sidecars` is a directory containing profiles and all other persistent files.
/// Stop its writers during creation for a consistent cross-file snapshot.
pub fn create(database: &Path, sidecars: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir(destination)?;
    let result = (|| {
        backup(database, &destination.join("catalog.sqlite"))?;
        let mut paths = Vec::new();
        collect(sidecars, Path::new(""), &mut paths)?;
        let mut manifest = fs::File::create(destination.join("manifest"))?;
        writeln!(manifest, "photo-culler-recovery-v1")?;
        let (n, h) = hash(&destination.join("catalog.sqlite"))?;
        writeln!(manifest, "{n}\t{h:016x}\tcatalog.sqlite")?;
        fs::create_dir(destination.join("files"))?;
        let count = paths.len() + 1;
        for rel in paths {
            let name = rel.to_str().ok_or_else(|| fail("non-UTF-8 sidecar path"))?;
            if name.contains(['\n', '\r', '\t']) {
                return Err(fail("unsupported sidecar path"));
            }
            let dst = destination.join("files").join(&rel);
            fs::create_dir_all(dst.parent().unwrap())?;
            fs::copy(sidecars.join(&rel), &dst)?;
            let (n, h) = hash(&dst)?;
            writeln!(manifest, "{n}\t{h:016x}\tfiles/{name}")?;
        }
        writeln!(manifest, "END\t{count}")?;
        manifest.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(destination);
    }
    result
}
/// Verify the whole snapshot before creating a new destination directory.
pub fn restore(snapshot: &Path, destination: &Path) -> io::Result<()> {
    let manifest = fs::read_to_string(snapshot.join("manifest"))?;
    let mut lines = manifest.lines();
    if lines.next() != Some("photo-culler-recovery-v1") {
        return Err(fail("invalid manifest"));
    }
    let mut paths = Vec::new();
    let mut end = None;
    for line in lines {
        if let Some(count) = line.strip_prefix("END\t") {
            if end.is_some() {
                return Err(fail("duplicate manifest footer"));
            }
            end = count.parse::<usize>().ok();
            continue;
        }
        if end.is_some() {
            return Err(fail("data after manifest footer"));
        }
        let mut fields = line.splitn(3, '\t');
        let n = fields
            .next()
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| fail("invalid length"))?;
        let h = fields
            .next()
            .and_then(|s| u64::from_str_radix(s, 16).ok())
            .ok_or_else(|| fail("invalid checksum"))?;
        let p = PathBuf::from(fields.next().ok_or_else(|| fail("invalid path"))?);
        if !safe_relative(&p)
            || !(p == Path::new("catalog.sqlite") || p.starts_with("files"))
            || paths.contains(&p)
        {
            return Err(fail("invalid manifest path"));
        }
        let full = snapshot.join(&p);
        let mut part = snapshot.to_path_buf();
        for c in p.components() {
            part.push(c);
            if fs::symlink_metadata(&part)?.file_type().is_symlink() {
                return Err(fail("snapshot contains symlink"));
            }
        }
        if hash(&full)? != (n, h) {
            return Err(fail("snapshot checksum mismatch"));
        }
        paths.push(p);
    }
    if end != Some(paths.len()) {
        return Err(fail("incomplete manifest"));
    }
    if !paths.contains(&PathBuf::from("catalog.sqlite")) {
        return Err(fail("snapshot missing catalog"));
    }
    integrity(&open(&snapshot.join("catalog.sqlite"), 1)?)?;
    fs::create_dir(destination)?;
    let result = (|| {
        fs::create_dir(destination.join("files"))?;
        for p in paths {
            let dst = destination.join(&p);
            fs::create_dir_all(dst.parent().unwrap())?;
            fs::copy(snapshot.join(p), dst)?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(destination);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sql(db: &Db, command: &str) {
        let sql = CString::new(command).unwrap();
        assert_eq!(
            unsafe { sqlite3_exec(db.0, sql.as_ptr(), None, ptr::null_mut(), ptr::null_mut()) },
            0
        );
    }
    #[test]
    fn wal_snapshot_restores_selection_recipe_and_profile_and_rejects_incomplete() {
        let root =
            std::env::temp_dir().join(format!("photo-culler-recovery-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).unwrap();
        let db = open(&root.join("source.sqlite"), 2 | 4).unwrap();
        sql(&db, "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE selection(id TEXT, recipe TEXT); INSERT INTO selection VALUES('selected-1','recipe-1');");
        assert!(root.join("source.sqlite-wal").exists());
        fs::create_dir(root.join("profiles")).unwrap();
        fs::write(root.join("profiles/profile.json"), "profile-1").unwrap();
        create(
            &root.join("source.sqlite"),
            &root.join("profiles"),
            &root.join("snapshot"),
        )
        .unwrap();
        restore(&root.join("snapshot"), &root.join("restored")).unwrap();
        let restored = open(&root.join("restored/catalog.sqlite"), 1).unwrap();
        unsafe extern "C" fn verify(
            arg: *mut c_void,
            n: c_int,
            vals: *mut *mut c_char,
            _: *mut *mut c_char,
        ) -> c_int {
            if n == 2
                && CStr::from_ptr(*vals).to_bytes() == b"selected-1"
                && CStr::from_ptr(*vals.add(1)).to_bytes() == b"recipe-1"
            {
                *(arg as *mut bool) = true;
            }
            0
        }
        let mut found = false;
        assert_eq!(
            unsafe {
                sqlite3_exec(
                    restored.0,
                    c"SELECT id, recipe FROM selection".as_ptr(),
                    Some(verify),
                    &mut found as *mut bool as *mut c_void,
                    ptr::null_mut(),
                )
            },
            0
        );
        assert!(found);
        assert_eq!(
            fs::read_to_string(root.join("restored/files/profile.json")).unwrap(),
            "profile-1"
        );
        fs::remove_file(root.join("snapshot/files/profile.json")).unwrap();
        assert!(restore(&root.join("snapshot"), &root.join("incomplete")).is_err());
        assert!(!root.join("incomplete").exists());
        drop(restored);
        drop(db);
        fs::remove_dir_all(root).unwrap();
    }
}

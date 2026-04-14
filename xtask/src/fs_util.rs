use std::fs::{self, File};

use cargo_metadata::camino::Utf8Path;
use color_eyre::eyre::{self, WrapErr as _, ensure};

pub(crate) fn create_file<P>(name: &str, path: P) -> eyre::Result<File>
where
    P: AsRef<Utf8Path>,
{
    let path = path.as_ref();
    File::create(path).wrap_err_with(|| format!("failed to create {name}: {path}"))
}

pub(crate) fn create_dir_all<P>(name: &str, path: P) -> eyre::Result<()>
where
    P: AsRef<Utf8Path>,
{
    let path = path.as_ref();
    fs::create_dir_all(path).wrap_err_with(|| format!("failed to create {name}: {path}"))
}

pub(crate) fn ensure_file_exists<P>(name: &str, path: P) -> eyre::Result<()>
where
    P: AsRef<Utf8Path>,
{
    let path = path.as_ref();
    let meta = path
        .metadata()
        .wrap_err_with(|| format!("failed to get metadata of {name}: {path}"))?;
    ensure!(meta.is_file(), "{name} is not a file: {path}");
    Ok(())
}

pub(crate) fn ensure_dir_exists<P>(name: &str, path: P) -> eyre::Result<()>
where
    P: AsRef<Utf8Path>,
{
    let path = path.as_ref();
    let meta = path
        .metadata()
        .wrap_err_with(|| format!("failed to get metadata of {name}: {path}"))?;
    ensure!(meta.is_dir(), "{name} is not a directory: {path}");
    Ok(())
}

pub(crate) fn copy<P, Q>(name: &str, src: P, dst: Q) -> eyre::Result<()>
where
    P: AsRef<Utf8Path>,
    Q: AsRef<Utf8Path>,
{
    let src = src.as_ref();
    let dst = dst.as_ref();

    ensure_file_exists(&format!("{name} source"), src)?;
    let dst_parent = dst.parent().ok_or_else(|| {
        eyre::eyre!("failed to get parent directory of {name} destination: {dst}")
    })?;
    ensure_dir_exists(&format!("{name} destination directory"), dst_parent)?;

    fs::copy(src, dst)
        .wrap_err_with(|| format!("failed to copy {name}:\n  src: {src}\n  dst: {dst}"))?;
    Ok(())
}

pub(crate) fn read_to_string<P>(name: &str, path: P) -> eyre::Result<String>
where
    P: AsRef<Utf8Path>,
{
    let path = path.as_ref();
    fs::read_to_string(path).wrap_err_with(|| format!("failed to read {name}: {path}"))
}

pub(crate) fn write<P, C>(name: &str, path: P, content: C) -> eyre::Result<()>
where
    P: AsRef<Utf8Path>,
    C: AsRef<[u8]>,
{
    let path = path.as_ref();
    let content = content.as_ref();
    fs::write(path, content).wrap_err_with(|| format!("failed to write {name}: {path}"))
}

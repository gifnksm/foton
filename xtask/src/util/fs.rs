use std::{
    fmt::Display,
    fs::{self, File},
};

use cargo_metadata::camino::Utf8Path;
use color_eyre::eyre::{self, WrapErr as _, ensure};
use serde::{Deserialize, Serialize};

pub(crate) fn create_file<N, P>(name: N, path: P) -> eyre::Result<File>
where
    N: Display,
    P: AsRef<Utf8Path>,
{
    let path = path.as_ref();
    File::create(path).wrap_err_with(|| format!("failed to create {name}: {path}"))
}

pub(crate) fn open_file<N, P>(name: N, path: P) -> eyre::Result<File>
where
    N: Display,
    P: AsRef<Utf8Path>,
{
    let path = path.as_ref();
    File::open(path).wrap_err_with(|| format!("failed to open {name}: {path}"))
}

pub(crate) fn create_dir_all<N, P>(name: N, path: P) -> eyre::Result<()>
where
    N: Display,
    P: AsRef<Utf8Path>,
{
    let path = path.as_ref();
    fs::create_dir_all(path).wrap_err_with(|| format!("failed to create {name}: {path}"))
}

pub(crate) fn ensure_file_exists<N, P>(name: N, path: P) -> eyre::Result<()>
where
    N: Display,
    P: AsRef<Utf8Path>,
{
    let path = path.as_ref();
    let meta = path
        .metadata()
        .wrap_err_with(|| format!("failed to get metadata of {name}: {path}"))?;
    ensure!(meta.is_file(), "{name} is not a file: {path}");
    Ok(())
}

pub(crate) fn ensure_dir_exists<N, P>(name: N, path: P) -> eyre::Result<()>
where
    N: Display,
    P: AsRef<Utf8Path>,
{
    let path = path.as_ref();
    let meta = path
        .metadata()
        .wrap_err_with(|| format!("failed to get metadata of {name}: {path}"))?;
    ensure!(meta.is_dir(), "{name} is not a directory: {path}");
    Ok(())
}

pub(crate) fn copy<N, P, Q>(name: N, src: P, dst: Q) -> eyre::Result<u64>
where
    N: Display,
    P: AsRef<Utf8Path>,
    Q: AsRef<Utf8Path>,
{
    let src = src.as_ref();
    let dst = dst.as_ref();

    ensure_file_exists(format_args!("{name} source"), src)?;
    let dst_parent = dst.parent().ok_or_else(|| {
        eyre::eyre!("failed to get parent directory of {name} destination: {dst}")
    })?;
    ensure_dir_exists(format_args!("{name} destination directory"), dst_parent)?;

    let bytes = fs::copy(src, dst)
        .wrap_err_with(|| format!("failed to copy {name}:\n  src: {src}\n  dst: {dst}"))?;
    Ok(bytes)
}

pub(crate) fn read_to_string<N, P>(name: N, path: P) -> eyre::Result<String>
where
    N: Display,
    P: AsRef<Utf8Path>,
{
    let path = path.as_ref();
    fs::read_to_string(path).wrap_err_with(|| format!("failed to read {name}: {path}"))
}

pub(crate) fn read_json<N, P, T>(name: N, path: P) -> eyre::Result<T>
where
    N: Display,
    P: AsRef<Utf8Path>,
    T: for<'a> Deserialize<'a>,
{
    let path = path.as_ref();
    let file = open_file(&name, path)?;
    let data = serde_json::from_reader(file)
        .wrap_err_with(|| format!("failed to read {name} as JSON: {path}"))?;
    Ok(data)
}

pub(crate) fn write<N, P, C>(name: N, path: P, content: C) -> eyre::Result<()>
where
    N: Display,
    P: AsRef<Utf8Path>,
    C: AsRef<[u8]>,
{
    let path = path.as_ref();
    let content = content.as_ref();
    fs::write(path, content).wrap_err_with(|| format!("failed to write {name}: {path}"))
}

pub(crate) fn write_json<N, P, T>(name: N, path: P, data: T) -> eyre::Result<()>
where
    N: Display,
    P: AsRef<Utf8Path>,
    T: Serialize,
{
    let path = path.as_ref();
    let mut file = create_file(&name, path)?;
    serde_json::to_writer_pretty(&mut file, &data)
        .wrap_err_with(|| format!("failed to write {name} as JSON: {path}"))?;
    Ok(())
}

use super::metadata::{Metadata, validate_identifier};
use anyhow::{Context, ensure};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

fn index_entry(name: &str) -> anyhow::Result<PathBuf> {
    validate_identifier(name, false)?;
    let name = name.to_ascii_lowercase();
    Ok(match name.len() {
        1 => format!("1/{name}"),
        2 => format!("2/{name}"),
        3 => format!("3/{}/{name}", &name[..1]),
        _ => format!("{}/{}/{name}", &name[..2], &name[2..4]),
    }
    .into())
}

fn copy(source: &Path, destination: &Path) -> anyhow::Result<()> {
    ensure!(
        source.is_file(),
        "missing public cache input: {}",
        source.display()
    );
    std::fs::create_dir_all(destination.parent().context("cache output has no parent")?)?;
    std::fs::copy(source, destination)
        .with_context(|| format!("copy public cache input {}", source.display()))?;
    Ok(())
}

struct Staging(PathBuf);
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) fn prepare(root: &Path, metadata: &Metadata) -> anyhow::Result<()> {
    let packages: Vec<_> = metadata
        .packages
        .iter()
        .filter(|package| package.source.is_some())
        .collect();
    let mut registries = BTreeSet::new();
    for package in &packages {
        validate_identifier(&package.name, false)?;
        validate_identifier(&package.version, true)?;
        ensure!(
            package
                .source
                .as_deref()
                .is_some_and(|source| source.starts_with("registry+")),
            "only public registry packages can enter the offline cache"
        );
        let directory = package
            .manifest_path
            .parent()
            .context("manifest has no parent")?;
        ensure!(
            directory
                .ancestors()
                .nth(2)
                .and_then(Path::file_name)
                .is_some_and(|name| name == "src")
                && directory
                    .ancestors()
                    .nth(3)
                    .and_then(Path::file_name)
                    .is_some_and(|name| name == "registry"),
            "package is not in a Cargo registry source cache: {}",
            package.name
        );
        let registry = directory
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .context("invalid registry directory")?;
        registries.insert(registry.to_owned());
    }
    ensure!(registries.len() == 1, "expected one registry source");
    let registry = registries.first().context("missing registry source")?;
    ensure!(
        registry.starts_with("rsproxy.cn-") || registry.starts_with("index.crates.io-"),
        "unsupported local registry; use the normal online Docker build"
    );
    let target = root.join("target");
    std::fs::create_dir_all(&target)?;
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        jiff::Timestamp::now().as_nanosecond()
    );
    let staging = Staging(target.join(format!(".docker-cargo-cache-{suffix}")));
    std::fs::create_dir(&staging.0)?;
    for package in packages {
        let directory = package
            .manifest_path
            .parent()
            .context("manifest has no parent")?;
        let registry_root = directory
            .ancestors()
            .nth(3)
            .context("registry root missing")?;
        let filename = format!("{}-{}.crate", package.name, package.version);
        copy(
            &registry_root.join("cache").join(registry).join(&filename),
            &staging
                .0
                .join("registry/cache")
                .join(registry)
                .join(filename),
        )?;
        let index = registry_root.join("index").join(registry);
        let destination = staging.0.join("registry/index").join(registry);
        copy(
            &index.join(".cache").join(index_entry(&package.name)?),
            &destination.join(".cache").join(index_entry(&package.name)?),
        )?;
        copy(&index.join("config.json"), &destination.join("config.json"))?;
    }
    if registry.starts_with("rsproxy.cn-") {
        std::fs::write(
            staging.0.join("config.toml"),
            "[source.crates-io]\nreplace-with = \"cached-registry\"\n[source.cached-registry]\nregistry = \"sparse+https://rsproxy.cn/index/\"\n",
        )?;
    }
    // Replace only the generated target cache; an incomplete preparation never touches it.
    let output = target.join("docker-cargo-cache");
    let previous = target.join(format!(".docker-cargo-cache-previous-{suffix}"));
    let had_previous = output.exists();
    if had_previous {
        std::fs::rename(&output, &previous)?;
    }
    if let Err(error) = std::fs::rename(&staging.0, &output) {
        if had_previous {
            std::fs::rename(&previous, &output).context("restore previous Docker cache")?;
        }
        return Err(error).context("publish Docker cache");
    }
    if had_previous {
        std::fs::remove_dir_all(previous)?;
    }
    println!("Public dependency cache prepared at target/docker-cargo-cache");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::{metadata::Package, test_support::Directory};
    #[test]
    fn sparse_index_paths_cover_every_name_length() {
        for (name, path) in [
            ("a", "1/a"),
            ("ab", "2/ab"),
            ("ABC", "3/a/abc"),
            ("cargo", "ca/rg/cargo"),
        ] {
            assert_eq!(index_entry(name).unwrap(), PathBuf::from(path));
        }
        assert!(index_entry("../../secret").is_err());
        assert!(index_entry("").is_err());
    }
    #[test]
    fn preparation_copies_public_inputs_and_removes_stale_configuration() {
        let dir = Directory::new();
        let registry = "index.crates.io-test";
        let base = dir.0.join("home/registry");
        let manifest = base
            .join("src")
            .join(registry)
            .join("example-1.0.0/Cargo.toml");
        std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        std::fs::write(&manifest, "").unwrap();
        let archive = base
            .join("cache")
            .join(registry)
            .join("example-1.0.0.crate");
        std::fs::create_dir_all(archive.parent().unwrap()).unwrap();
        std::fs::write(archive, "public archive").unwrap();
        let index = base.join("index").join(registry);
        let entry = index.join(".cache").join(index_entry("example").unwrap());
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(entry, "public index").unwrap();
        std::fs::write(index.join("config.json"), "{}").unwrap();
        std::fs::write(dir.0.join("home/credentials.toml"), "must not be copied").unwrap();
        let output = dir.0.join("target/docker-cargo-cache");
        std::fs::create_dir_all(&output).unwrap();
        std::fs::write(output.join("config.toml"), "stale mirror").unwrap();
        let metadata = Metadata {
            packages: vec![Package {
                name: "example".into(),
                version: "1.0.0".into(),
                license: Some("MIT".into()),
                manifest_path: manifest,
                source: Some("registry+https://github.com/rust-lang/crates.io-index".into()),
            }],
        };
        prepare(&dir.0, &metadata).unwrap();
        assert!(!output.join("config.toml").exists());
        assert!(!output.join("credentials.toml").exists());
        assert_eq!(
            std::fs::read_to_string(
                output
                    .join("registry/cache")
                    .join(registry)
                    .join("example-1.0.0.crate")
            )
            .unwrap(),
            "public archive"
        );
    }

    #[test]
    fn missing_archive_preserves_previous_cache_and_removes_staging() {
        let dir = Directory::new();
        let registry = "index.crates.io-test";
        let manifest = dir
            .0
            .join("home/registry/src")
            .join(registry)
            .join("example-1.0.0/Cargo.toml");
        std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        let output = dir.0.join("target/docker-cargo-cache");
        std::fs::create_dir_all(&output).unwrap();
        std::fs::write(output.join("previous"), "last successful cache").unwrap();
        let metadata = Metadata {
            packages: vec![Package {
                name: "example".into(),
                version: "1.0.0".into(),
                license: Some("MIT".into()),
                manifest_path: manifest,
                source: Some("registry+https://github.com/rust-lang/crates.io-index".into()),
            }],
        };
        assert!(prepare(&dir.0, &metadata).is_err());
        assert_eq!(
            std::fs::read_to_string(output.join("previous")).unwrap(),
            "last successful cache"
        );
        assert_eq!(std::fs::read_dir(dir.0.join("target")).unwrap().count(), 1);
    }
}

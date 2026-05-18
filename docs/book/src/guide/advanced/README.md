# Advanced Usage

This section is for users who want to do more than install packages from the
public package registry.

Typical advanced workflows include:

- writing your own package manifest
- validating that manifest before publishing or installing it
- testing a manifest locally with `foton install --manifest`
- setting up a custom package registry
- using local and Git-backed package registries from your configuration

## Suggested workflow

If you are creating a package, a typical workflow looks like this:

1. Write a package manifest
2. Run `foton manifest check` to validate it
3. Test it locally with `foton install --manifest`
4. Add it to a package registry if you want to distribute it

## Chapters in this section

- [Writing a Package Manifest](write-package-manifest.md)
- [Setting Up Your Own Package Registry](setup-package-registry.md)

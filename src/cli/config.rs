#[derive(Debug, Default)]
pub(crate) struct Config {
    pub(crate) install: InstallConfig,
}

#[derive(Debug)]
#[expect(clippy::struct_field_names)]
pub(crate) struct InstallConfig {
    pub(crate) max_archive_size_bytes: u64,
    pub(crate) max_extracted_files: usize,
    pub(crate) max_extracted_file_size_bytes: u64,
}

impl Default for InstallConfig {
    fn default() -> Self {
        Self {
            max_archive_size_bytes: 100 * 1024 * 1024, // 100 MiB
            max_extracted_files: 1000,
            max_extracted_file_size_bytes: 50 * 1024 * 1024, // 50 MiB
        }
    }
}

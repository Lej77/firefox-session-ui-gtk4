#![cfg_attr(
    not(feature = "real_data"),
    allow(dead_code, unused_variables, unused_imports)
)]

use std::{
    borrow::Cow, env, fs::OpenOptions, io::Empty, path::PathBuf, sync::Arc, time::SystemTime,
};

use either::Either;
#[cfg(feature = "real_data")]
use firefox_session_data::session_store::FirefoxSessionStore;
#[cfg(feature = "real_data")]
pub use firefox_session_data::to_links::ttl_formats::FormatInfo;

#[cfg(not(feature = "real_data"))]
mod fake {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum FormatInfo {
        PDF,
        Text,
        Html,
    }
    impl FormatInfo {
        pub fn all() -> &'static [Self] {
            &[Self::PDF, Self::Text, Self::Html]
        }
        pub fn as_str(&self) -> &'static str {
            match self {
                FormatInfo::PDF => "pdf",
                FormatInfo::Text => "text",
                FormatInfo::Html => "html",
            }
        }
    }
    impl std::fmt::Display for FormatInfo {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                FormatInfo::PDF => write!(f, "Convert the links to a PDF that contains links."),
                FormatInfo::Text => write!(f, "Convert the links to raw text."),
                FormatInfo::Html => write!(f, "Convert the links to a HTML file with anchor tags."),
            }
        }
    }

    pub(super) type FirefoxSessionStore = ();

    impl FileInfo {
        pub async fn load_data(&mut self) -> Result<(), String> {
            self.data = Some(FileData::Compressed(Arc::new([])));
            Ok(())
        }

        pub async fn decompress_data(&mut self) -> Result<(), String> {
            self.data = Some(FileData::Uncompressed(Arc::new([])));
            Ok(())
        }

        pub async fn parse_session_data(&mut self) -> Result<(), String> {
            self.data = Some(FileData::Parsed(Arc::new(())));
            Ok(())
        }

        pub async fn get_groups_from_session(
            &self,
            sort_groups: bool,
        ) -> Result<AllTabGroups, String> {
            Ok(AllTabGroups {
                open: vec![
                    TabGroup {
                        index: 0,
                        name: "Window 1".into(),
                    },
                    TabGroup {
                        index: 1,
                        name: "Window 2".into(),
                    },
                ],
                closed: vec![TabGroup {
                    index: 2,
                    name: "Closed window 1".into(),
                }],
            })
        }

        pub async fn to_text_links(
            &self,
            generate_options: GenerateOptions,
        ) -> Result<String, String> {
            Ok("http://www.example.com".to_string())
        }

        pub async fn save_links(
            &self,
            mut save_path: PathBuf,
            generate_options: GenerateOptions,
            output_options: OutputOptions,
        ) -> Result<(), String> {
            Ok(())
        }
    }
}
#[cfg(not(feature = "real_data"))]
pub use fake::*;

/// Attempt to present markdown nicely.
pub fn render_markdown(markdown: &str) -> String {
    markdown
        // Ensure there are no \r characters in the string:
        .replace("\r", "")
        // Handle lists gracefully:
        .replace("\n-", "\r-")
        .replace("\n -", "\r -")
        // Keep double newlines:
        .replace("\n\n", "\r")
        // Replace newlines with spaces:
        .replace("\n", " ")
        // \r should be actual newlines:
        .replace("\r", "\n")
}

#[derive(Debug, Clone)]
pub struct FirefoxProfileInfo {
    path: PathBuf,
    #[expect(dead_code, reason = "we don't expose this field currently")]
    modified_at: Result<SystemTime, String>,
}
impl FirefoxProfileInfo {
    /// Name of the Firefox profile folder.
    pub fn name(&self) -> Cow<'_, str> {
        self.path.file_name().unwrap_or_default().to_string_lossy()
    }
    pub fn find_sessionstore_file(&self) -> PathBuf {
        let previous = self.path.join("sessionstore-backups/previous.jsonlz4");
        let recovery_older = self.path.join("sessionstore-backups/recovery.baklz4");
        let recovery = self.path.join("sessionstore-backups/recovery.jsonlz4");
        let session_store = self.path.join("sessionstore.jsonlz4");

        if session_store.exists() {
            // If Firefox is closed then this has the latest data:
            session_store
        } else if recovery.exists() {
            // Otherwise this keeps the latest data:
            recovery
        } else if recovery_older.exists() {
            // When Firefox is overwriting recovery it moves
            // the old one here first:
            recovery_older
        } else if previous.exists() {
            // And this is from the previous startup:
            previous
        } else {
            // This is written most often so keep trying it:
            recovery
        }
    }
    pub fn all_profiles() -> Vec<FirefoxProfileInfo> {
        #[cfg(feature = "real_data")]
        let profiles = ::firefox_session_data::find::FirefoxProfileFinder::new()
            .and_then(|finder| {
                Ok(finder
                    .all_profiles()?
                    .iter()
                    .map(|(p, t)| (p.clone(), t.as_ref().map_err(|e| e.to_string()).copied()))
                    .map(|(path, modified_at)| FirefoxProfileInfo { path, modified_at })
                    .collect::<Vec<_>>())
            })
            .unwrap_or_default();

        #[cfg(not(feature = "real_data"))]
        let profiles: Vec<FirefoxProfileInfo> = vec![FirefoxProfileInfo {
            path: "./firefox-profiles/02921.default-release",
            modified_at: Err("Not available".to_string()),
        }];

        profiles
    }
}

pub async fn prompt_load_file() -> Option<PathBuf> {
    let mut builder = ::rfd::AsyncFileDialog::new() //.set_parent(&**cx)
        .add_filter("Firefox session file", &["js", "baklz4", "jsonlz4"])
        .add_filter("All files", &["*"])
        .set_title("Open Firefox Sessionstore File");

    if let Some(data) = env::var_os("APPDATA") {
        let data = PathBuf::from(data);
        builder = builder.set_directory(data.join("Mozilla\\Firefox\\Profiles"));
    }

    let file_path = {
        let handle = builder.pick_file().await?;
        handle.path().to_owned()
    };

    Some(file_path)
}

pub async fn prompt_save_file() -> Option<PathBuf> {
    let builder = rfd::AsyncFileDialog::new()
        // .set_parent(&**cx)
        // .add_filter("All files", &["*"])
        .set_title("Save Links from Firefox Tabs");

    let handle = builder.save_file().await?;
    Some(handle.path().to_owned())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabGroup {
    /// The index of the tab group. Used to uniquely identify a group in a
    /// specific session.
    pub index: u32,
    /// Name of a tab group.
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AllTabGroups {
    /// Tab groups or windows.
    pub open: Vec<TabGroup>,
    /// Tab groups or windows in recently closed windows.
    pub closed: Vec<TabGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerateOptions {
    pub open_group_indexes: Option<Vec<u32>>,
    pub closed_group_indexes: Option<Vec<u32>>,
    pub sort_groups: bool,
    pub table_of_content: bool,
}
impl GenerateOptions {
    pub fn selected_groups(&self) -> usize {
        self.open_group_indexes.as_ref().map_or(0, Vec::len)
            + self.closed_group_indexes.as_ref().map_or(0, Vec::len)
    }
}
impl Default for GenerateOptions {
    fn default() -> Self {
        Self {
            open_group_indexes: None,
            closed_group_indexes: Some(Vec::new()),
            sort_groups: true,
            table_of_content: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputOptions {
    pub format: FormatInfo,
    pub overwrite: bool,
    pub create_folder: bool,
}

impl Default for OutputOptions {
    fn default() -> Self {
        Self {
            format: FormatInfo::PDF,
            overwrite: Default::default(),
            create_folder: Default::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum FileData {
    Compressed(Arc<[u8]>),
    Uncompressed(Arc<[u8]>),
    Parsed(Arc<FirefoxSessionStore>),
}
impl FileData {
    pub fn as_parsed(&self) -> Option<&Arc<FirefoxSessionStore>> {
        if let Self::Parsed(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileInfo {
    pub file_path: Arc<PathBuf>,
    pub data: Option<FileData>,
}
impl FileInfo {
    pub fn new(file_path: PathBuf) -> Self {
        Self {
            file_path: Arc::new(file_path),
            data: None,
        }
    }
    pub fn is_compressed_file_format(&self) -> bool {
        self.file_path
            .extension()
            .and_then(|ext| ext.to_str().map(|v| v.ends_with("lz4")))
            .unwrap_or(false)
    }
}
#[cfg(feature = "real_data")]
impl FileInfo {
    pub async fn load_data(&mut self) -> Result<(), String> {
        use std::{
            fs::File,
            io::{BufReader, Read},
        };
        if self.data.is_some() {
            return Ok(());
        }

        let path = self.file_path.clone();

        let data = tokio::task::spawn_blocking(move || -> Result<_, String> {
            let file = File::open(&*path)
                .map_err(|e| format!("failed to open file at {}: {e}", path.display()))?;

            let mut buffer = BufReader::new(file);
            let mut data = Vec::new();

            buffer
                .read_to_end(&mut data)
                .map_err(|e| format!("failed to read file data from {}: {e}", path.display()))?;

            Ok(data)
        })
        .await
        .expect("panicked when reading session data")?;

        let data = Arc::from(data);
        self.data = Some(if self.is_compressed_file_format() {
            FileData::Compressed(data)
        } else {
            FileData::Uncompressed(data)
        });

        Ok(())
    }
    pub async fn decompress_data(&mut self) -> Result<(), String> {
        let data = match self
            .data
            .as_ref()
            .ok_or("Tried to decompress data before reading it")?
        {
            FileData::Compressed(data) => data.clone(),
            FileData::Uncompressed(_) | FileData::Parsed(_) => return Ok(()),
        };
        let decompressed = tokio::task::spawn_blocking(move || {
            firefox_session_data::io_utils::decompress_lz4_data(Either::<_, Empty>::Left(
                Vec::<u8>::from(&*data).into(),
            ))
            .map(|reader| -> Vec<u8> { reader.into() })
            .map_err(|e| format!("failed to decompress data: {e}"))
        })
        .await
        .unwrap()?;

        self.data = Some(FileData::Uncompressed(Arc::from(decompressed)));
        Ok(())
    }
    pub async fn parse_session_data(&mut self) -> Result<(), String> {
        let data = match self
            .data
            .as_ref()
            .ok_or("Tried to parse data before reading it")?
        {
            FileData::Compressed(_) => return Err("can't parse compressed data".to_string()),
            FileData::Uncompressed(data) => data.clone(),
            FileData::Parsed(_) => return Ok(()),
        };
        let session = tokio::task::spawn_blocking(move || {
            serde_json::from_slice::<FirefoxSessionStore>(&data)
                .map_err(|e| format!("failed to parse sessionstore JSON data: {e}"))
        })
        .await
        .map_err(|e| format!("deserialization of JSON sessionstore data panicked: {e}"))??;

        self.data = Some(FileData::Parsed(Arc::new(session)));
        Ok(())
    }
    pub async fn get_groups_from_session(&self, sort_groups: bool) -> Result<AllTabGroups, String> {
        use firefox_session_data::session_store::session_info::get_groups_from_session;

        let session = self
            .data
            .as_ref()
            .and_then(FileData::as_parsed)
            .cloned()
            .ok_or("must deserialize JSON sessionstore data before tab groups can be inspected")?;

        Ok(tokio::task::spawn_blocking(move || AllTabGroups {
            open: get_groups_from_session(&session, true, false, sort_groups)
                .enumerate()
                .map(|(ix, group)| TabGroup {
                    index: ix as _,
                    name: group.name().to_owned(),
                })
                .collect::<Vec<_>>(),
            closed: get_groups_from_session(&session, false, true, sort_groups)
                .enumerate()
                .map(|(ix, group)| TabGroup {
                    index: ix as _,
                    name: group.name().to_owned(),
                })
                .collect::<Vec<_>>(),
        })
        .await
        .unwrap())
    }

    /// Generate a text only representation of the sessionstore data.
    pub async fn to_text_links(&self, generate_options: GenerateOptions) -> Result<String, String> {
        use firefox_session_data::{
            pdf_converter::html_to_pdf::WriteBuilderSimple,
            session_store::{
                session_info::{get_groups_from_session, TreeDataSource},
                to_links::LinkFormat,
                to_links::ToLinksOptions,
            },
            to_links::TabsToLinksOutput,
        };

        let session = self
            .data
            .as_ref()
            .and_then(FileData::as_parsed)
            .cloned()
            .ok_or("must deserialize JSON sessionstore data before converting tabs to links")?;

        tokio::task::spawn_blocking(move || {
            let mut output: Vec<u8> = Vec::new();

            let open_groups =
                get_groups_from_session(&session, true, false, generate_options.sort_groups)
                    .enumerate()
                    .filter(|(ix, _)| {
                        if let Some(indexes) = &generate_options.open_group_indexes {
                            indexes.contains(&(*ix as u32))
                        } else {
                            true
                        }
                    })
                    .map(|(_, g)| g);

            let closed_groups =
                get_groups_from_session(&session, false, true, generate_options.sort_groups)
                    .enumerate()
                    .filter(|(ix, _)| {
                        if let Some(indexes) = &generate_options.closed_group_indexes {
                            indexes.contains(&(*ix as u32))
                        } else {
                            true
                        }
                    })
                    .map(|(_, g)| g);

            firefox_session_data::tabs_to_links(
                &open_groups.chain(closed_groups).collect::<Vec<_>>(),
                TabsToLinksOutput {
                    format: LinkFormat::TXT,
                    as_pdf: None,
                    conversion_options: ToLinksOptions {
                        format: LinkFormat::TXT,
                        page_breaks_after_group: false, // We don't have any page break character in raw text
                        skip_page_break_after_last_group: true,
                        table_of_contents: generate_options.table_of_content,
                        indent_all_links: true,
                        custom_page_break: "".into(),
                        // If there is any data from Sidebery then TST data
                        // won't be used and so on:
                        tree_sources: (&[
                            TreeDataSource::Sidebery,
                            TreeDataSource::TstWebExtension,
                            TreeDataSource::TstLegacy,
                        ] as &[_])
                            .into(),
                    },
                },
                WriteBuilderSimple(&mut output),
            )
            .map_err(|e| e.to_string())?;

            Ok(String::from_utf8_lossy(&output).into_owned())
        })
        .await
        .unwrap()
    }
    pub async fn save_links(
        &self,
        mut save_path: PathBuf,
        generate_options: GenerateOptions,
        output_options: OutputOptions,
    ) -> Result<(), String> {
        use firefox_session_data::{
            pdf_converter::html_to_pdf::WriteBuilderSimple,
            session_store::{
                session_info::{get_groups_from_session, TreeDataSource},
                to_links::LinkFormat,
                to_links::ToLinksOptions,
            },
            to_links::TabsToLinksOutput,
        };

        let session = self
            .data
            .as_ref()
            .and_then(FileData::as_parsed)
            .cloned()
            .ok_or("must deserialize JSON sessionstore data before converting tabs to links")?;

        tokio::task::spawn_blocking(move || {
            let (format, as_pdf) = output_options.format.as_format().to_link_format();
            let file_ext = if as_pdf.is_some() {
                "pdf"
            } else {
                match format {
                    LinkFormat::TXT => "txt",
                    LinkFormat::RTF { .. } => "rtf",
                    LinkFormat::HTML => "html",
                    LinkFormat::Markdown => "md",
                    LinkFormat::Typst => "typ",
                }
            };
            if save_path.extension().is_none() {
                save_path.set_extension(file_ext);
            }

            if let Some(folder) = save_path.parent() {
                if output_options.create_folder {
                    std::fs::create_dir_all(folder).map_err(|e| {
                        format!("failed to create folder at \"{}\": {e}", folder.display())
                    })?;
                }
            }
            let mut file = OpenOptions::new()
                .write(true)
                .truncate(true)
                .create(true)
                .create_new(!output_options.overwrite)
                .open(&save_path)
                .map_err(|e| {
                    format!(
                        "failed to create new file at \"{}\": {e}",
                        save_path.display()
                    )
                })?;

            let open_groups =
                get_groups_from_session(&session, true, false, generate_options.sort_groups)
                    .enumerate()
                    .filter(|(ix, _)| {
                        if let Some(indexes) = &generate_options.open_group_indexes {
                            indexes.contains(&(*ix as u32))
                        } else {
                            true
                        }
                    })
                    .map(|(_, g)| g);

            let closed_groups =
                get_groups_from_session(&session, false, true, generate_options.sort_groups)
                    .enumerate()
                    .filter(|(ix, _)| {
                        if let Some(indexes) = &generate_options.closed_group_indexes {
                            indexes.contains(&(*ix as u32))
                        } else {
                            true
                        }
                    })
                    .map(|(_, g)| g);

            let page_breaks = !matches!(output_options.format, FormatInfo::TEXT);
            firefox_session_data::tabs_to_links(
                &open_groups.chain(closed_groups).collect::<Vec<_>>(),
                TabsToLinksOutput {
                    format,
                    as_pdf,
                    conversion_options: ToLinksOptions {
                        format,
                        page_breaks_after_group: page_breaks,
                        skip_page_break_after_last_group: page_breaks && format.is_html(),
                        table_of_contents: generate_options.table_of_content,
                        indent_all_links: true,
                        custom_page_break: "".into(),
                        // If there is any data from Sidebery then TST data
                        // won't be used and so on:
                        tree_sources: (&[
                            TreeDataSource::Sidebery,
                            TreeDataSource::TstWebExtension,
                            TreeDataSource::TstLegacy,
                        ] as &[_])
                            .into(),
                    },
                },
                WriteBuilderSimple(&mut file),
            )
            .map_err(|e| e.to_string())?;

            Ok(())
        })
        .await
        .unwrap()
    }
}

mod actions;
#[cfg(feature = "dark-theme")]
mod dark_theme;

use std::{any::Any, future::Future, path::PathBuf, pin::Pin, time::Duration};

use actions::{FileData, FileInfo};
use gio::ApplicationFlags;
use gtk::{gdk, gio, prelude::*};
use relm4::{
    gtk,
    prelude::{DynamicIndex, FactoryComponent, FactoryVecDeque},
    Component, ComponentParts, ComponentSender, FactorySender, RelmApp, RelmWidgetExt,
};

#[derive(Debug)]
struct RowLabelModel {
    pub name: String,
    pub activatable: bool,
    pub selectable: bool,
    pub opacity: f64,
    pub data: Box<dyn Any>,
}
impl Default for RowLabelModel {
    fn default() -> Self {
        Self {
            name: String::new(),
            activatable: true,
            selectable: true,
            opacity: 1.0,
            data: Box::new(()),
        }
    }
}
#[relm4::factory]
impl FactoryComponent for RowLabelModel {
    type Init = RowLabelModel;
    type Input = ();
    type Output = ();
    type CommandOutput = ();
    type ParentWidget = gtk::ListBox;

    view! {
        #[root]
        gtk::ListBoxRow {
            set_selectable: self.selectable,
            set_activatable: self.activatable,
            gtk::Label {
                set_label: &self.name,
                set_opacity: self.opacity,
            },
        }
    }

    fn init_model(value: Self::Init, _index: &DynamicIndex, _sender: FactorySender<Self>) -> Self {
        value
    }

    fn update(&mut self, _msg: Self::Input, _sender: FactorySender<Self>) {}
}

/// A boxed future that implements `Send`.
type BoxedFuture<T = ()> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

struct AppModel {
    sidebar_indexes: Vec<i32>,
    /// Index of last activated sidebar item. Used when handling
    /// [`AppMsg::SidebarRowsChanged`].
    sidebar_activated_temp: Option<i32>,
    /// Number of programmatically queued row activations, these should be
    /// ignored when handling [`AppMsg::SidebarRowSelected`].
    sidebar_queued_activations: u32,
    sidebar_list_items: FactoryVecDeque<RowLabelModel>,

    input_path: PathBuf,
    loaded_data: Option<FileInfo>,
    tab_groups: actions::AllTabGroups,
    output_path: PathBuf,
    output_format: actions::FormatInfo,
    status: String,
    is_showing_wizard: bool,
    wizard_profiles: FactoryVecDeque<RowLabelModel>,
    background_worker: tokio::sync::mpsc::UnboundedSender<BoxedFuture>,
}
impl AppModel {
    /// Set a path to a user input text. This should be careful with updating
    /// the path if the actual path is not UTF-8.
    fn update_path(path: &mut PathBuf, text: &str) {
        if *path.to_string_lossy() != *text {
            *path = PathBuf::from(text);
        }
    }
    fn update_sidebar_list(&mut self) {
        {
            let mut list_items = self.sidebar_list_items.guard();
            list_items.clear();

            list_items.push_back(RowLabelModel {
                name: "Open Windows:".to_owned(),
                activatable: false,
                selectable: false,
                opacity: 0.6,
                ..Default::default()
            });
            for group in &self.tab_groups.open {
                list_items.push_back(RowLabelModel {
                    name: group.name.clone(),
                    data: Box::new((true, group.clone())),
                    ..Default::default()
                });
            }

            list_items.push_back(RowLabelModel {
                activatable: false,
                selectable: false,
                ..Default::default()
            });
            list_items.push_back(RowLabelModel {
                name: "Closed Windows:".to_owned(),
                activatable: false,
                selectable: false,
                opacity: 0.6,
                ..Default::default()
            });
            for group in &self.tab_groups.closed {
                list_items.push_back(RowLabelModel {
                    name: group.name.clone(),
                    data: Box::new((false, group.clone())),
                    ..Default::default()
                });
            }
        }
    }
    /// The currently selected windows or tab groups that the user want to
    /// preview and later write to a file.
    fn selected_tab_groups(
        &self,
        widgets: &<Self as Component>::Widgets,
    ) -> actions::GenerateOptions {
        let mut options = actions::GenerateOptions {
            open_group_indexes: None,
            closed_group_indexes: Some(Vec::new()),
            sort_groups: true,
            table_of_content: true,
        };
        widgets.sidebar_list.selected_foreach(|_, row| {
            let Some(model) = self.sidebar_list_items.get(row.index() as usize) else {
                return;
            };
            let (open, group) = model
                .data
                .downcast_ref::<(bool, actions::TabGroup)>()
                .expect("failed to downcast sidebar list data");

            let open_groups = options.open_group_indexes.get_or_insert_with(Vec::new);
            let closed_groups = options.closed_group_indexes.get_or_insert_with(Vec::new);
            let indexes = if *open { open_groups } else { closed_groups };
            indexes.push(group.index);
        });
        options
    }

    /// Queue some work to be preformed later. The queued work will not be
    /// executed if a newer action is queued.
    fn queue_background_work(&self, work: impl Future<Output = ()> + Send + 'static) {
        self.background_worker
            .send(Box::pin(work))
            .expect("failed to spawn background work");
    }
    /// Main loop of background work task that ensures only a single background
    /// task is executed at any time.
    async fn background_work_loop(mut rx: tokio::sync::mpsc::UnboundedReceiver<BoxedFuture>) {
        while let Some(mut work) = rx.recv().await {
            // Wait for other actions to be queued:
            tokio::time::sleep(Duration::from_millis(5)).await;

            // Only preform the last queued action:
            while let Ok(newer_work) = rx.try_recv() {
                // Ignore work that has been canceled.
                work = newer_work;
            }

            // Preform the queued work:
            work.await;
        }
    }
}

/// These messages are generated by user input or changes to the GUI.
#[derive(Debug)]
enum AppInputMsg {
    WindowShow,
    SidebarRowSelected(i32),
    SidebarRowsChanged,
    EditedInputPath,
    OpenWizard,
    CloseWizard,
    SelectedWizardProfile(i32),
    BrowseInput,
    LoadNewData,
    EditedOutputPath,
    BrowseOutput,
    OutputFormatChanged,
    CopyLinksToClipboard,
    SaveLinksToFile,
    PreviewChanged,
}
/// These messages are generated by background tasks.
#[derive(Debug)]
enum AppCommandMsg {
    SetInputPath(PathBuf),
    SetOutputPath(PathBuf),
    UpdateLoadedData(FileInfo),
    ParsedTabGroups(actions::AllTabGroups),
    RegeneratePreview,
    SetPreview(String),
    SetStatus(String),
    FixPreviewScrollbar,
}

#[relm4::component]
impl Component for AppModel {
    type Init = u8;

    type Input = AppInputMsg;
    type Output = ();
    type CommandOutput = AppCommandMsg;

    view! {
        #[name(window)]
        gtk::ApplicationWindow {
            set_title: Some("Firefox Session Data Utility"),
            set_default_width: 1000,
            set_default_height: 700,

            connect_show => AppInputMsg::WindowShow,

            #[name = "wizard_container"]
            gtk::Overlay {
                add_overlay = &gtk::Box {
                    // Covers the whole window
                    set_orientation: gtk::Orientation::Vertical,
                    #[watch]
                    set_visible: model.is_showing_wizard,
                    add_css_class: "overlay-background",

                    // Close wizard on background click:
                    // https://stackoverflow.com/questions/69891299/making-a-clickable-box-in-gtk4
                    add_controller = gtk::GestureClick::new() {
                        connect_released[sender] => move |gesture, _, _, _| {
                            gesture.set_state(gtk::EventSequenceState::Claimed);
                            sender.input(AppInputMsg::CloseWizard);
                        }
                    },

                    gtk::Frame {
                        // Covers mostly the whole height and centered horizontally
                        set_halign: gtk::Align::Center,

                        set_vexpand: true,
                        set_valign: gtk::Align::Fill,

                        add_css_class: "wizard-dialog",
                        set_margin_all: 50,

                        // Prevent click events from bubbling out of this widget:
                        add_controller = gtk::GestureClick::new() {
                            connect_released => move |gesture, _, _, _| {
                                gesture.set_state(gtk::EventSequenceState::Claimed);
                            }
                        },

                        #[wrap(Some)]
                        set_label_widget = &gtk::Label {
                            set_label: "Select Firefox Session Data",
                            add_css_class: "wizard-header",
                        },
                        gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            gtk::Label {
                                set_label: "Firefox Profiles:",
                                set_halign: gtk::Align::Start,
                                set_margin_top: 10,
                                set_margin_bottom: 5,
                            },

                            gtk::ScrolledWindow {
                                set_vexpand: true,
                                set_valign: gtk::Align::Fill,

                                #[local_ref]
                                wizard_profile_list -> gtk::ListBox {
                                    set_width_request: 200,
                                    set_activate_on_single_click: true,
                                    set_selection_mode: gtk::SelectionMode::Browse,

                                    connect_row_activated[sender] => move |_list, row| {
                                        sender.input(AppInputMsg::SelectedWizardProfile(row.index()));
                                        sender.input(AppInputMsg::CloseWizard);
                                    },
                                },
                            }
                        }
                    }
                },
                gtk::Box {
                    set_orientation: gtk::Orientation::Horizontal,
                    set_spacing: 5,
                    set_margin_all: 5,

                    // Sidebar with list of windows:
                    gtk::ScrolledWindow {
                        set_width_request: 200,
                        #[local_ref]
                        sidebar_list -> gtk::ListBox {
                            set_activate_on_single_click: false,
                            set_selection_mode: gtk::SelectionMode::Multiple,
                            connect_row_selected[sender] => move |_, row| {
                                if let Some(row) = row {
                                    sender.input(AppInputMsg::SidebarRowSelected(row.index()));
                                }
                            },
                            connect_selected_rows_changed => AppInputMsg::SidebarRowsChanged,
                        },
                    },
                    // Main content:
                    gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                        set_spacing: 5,
                        set_margin_all: 5,

                        gtk::Box {
                            set_orientation: gtk::Orientation::Horizontal,
                            set_spacing: 5,
                            set_margin_all: 5,

                            gtk::Label {
                                set_label: "Path to sessionstore file:",
                            },
                            #[name = "input_path"]
                            gtk::Entry {
                                set_hexpand: true,
                                set_halign: gtk::Align::Fill,
                                connect_changed => AppInputMsg::EditedInputPath,
                                set_text: &model.input_path.to_string_lossy(),
                            },
                            gtk::Button {
                                set_label: "Wizard",
                                connect_clicked => AppInputMsg::OpenWizard,
                            },
                            gtk::Button {
                                set_label: "Browse",
                                connect_clicked => AppInputMsg::BrowseInput,
                            },
                        },

                        gtk::Box {
                            set_orientation: gtk::Orientation::Horizontal,
                            set_spacing: 5,
                            set_margin_all: 5,

                            gtk::Label {
                                set_label: "Current data was loaded from:",
                            },
                            #[name = "loaded_path"]
                            gtk::Entry {
                                set_hexpand: true,
                                set_halign: gtk::Align::Fill,
                                set_editable: false,
                                #[watch]
                                set_text: &model
                                    .loaded_data
                                    .as_ref()
                                    .map(|data| data.file_path.to_string_lossy())
                                    .unwrap_or_default(),
                            },
                            gtk::Button {
                                set_label: "Load new data",
                                connect_clicked => AppInputMsg::LoadNewData,
                            }
                        },

                        gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_spacing: 5,
                            set_margin_all: 5,

                            gtk::Label {
                                set_label: "Tabs as Links:",
                                set_halign: gtk::Align::Start,
                            },
                            #[name = "preview_scrolled_window"]
                            gtk::ScrolledWindow {
                                set_valign: gtk::Align::Fill,
                                set_vexpand: true,
                                // Alternate way to create scrollable text view (worse in most ways):
                                // gtk::Viewport { #[wrap(Some)] set_child: preview = &gtk::TextView { set_editable: false, }, },
                                #[name = "preview"]
                                gtk::TextView {
                                    set_editable: false,
                                },
                            }
                        },

                        gtk::Box {
                            set_orientation: gtk::Orientation::Horizontal,
                            set_spacing: 5,
                            set_margin_all: 5,

                            gtk::Label {
                                set_label: "File path to write links to:",
                            },
                            #[name = "output_path"]
                            gtk::Entry {
                                set_hexpand: true,
                                set_halign: gtk::Align::Fill,
                                connect_changed => AppInputMsg::EditedOutputPath,
                                set_text: &model.output_path.to_string_lossy(),
                            },
                            gtk::Button {
                                set_label: "Browse",
                                connect_clicked => AppInputMsg::BrowseOutput,
                            }
                        },

                        gtk::Box {
                            set_orientation: gtk::Orientation::Horizontal,
                            set_spacing: 5,
                            set_margin_all: 5,

                            #[name = "should_create_folder"]
                            gtk::CheckButton::with_label("Create folder if it does not exist") {},

                            #[name = "should_overwrite"]
                            gtk::CheckButton::with_label("Overwrite file if it already exists") {},
                        },

                        gtk::Box {
                            set_orientation: gtk::Orientation::Horizontal,
                            set_spacing: 5,
                            set_margin_all: 5,

                            gtk::Button {
                                set_label: "Copy links to clipboard",
                                connect_clicked => AppInputMsg::CopyLinksToClipboard,
                            },

                            gtk::Box {
                                // This is used as a "spacer" to align the buttons to the right
                                set_hexpand: true,
                                set_halign: gtk::Align::Fill,
                            },

                            #[name = "output_format"]
                            gtk::DropDown::from_strings(
                                &actions::FormatInfo::all()
                                    .iter()
                                    .map(|v| v.as_str())
                                    .collect::<Vec<_>>()
                            ) {
                                set_selected: actions::FormatInfo::all()
                                    .iter()
                                    .position(|&item| item == model.output_format)
                                    .unwrap_or(0) as u32,
                                #[watch]
                                set_tooltip: &actions::render_markdown(&model.output_format.to_string()),
                                connect_selected_item_notify => AppInputMsg::OutputFormatChanged,
                            },

                            gtk::Button {
                                set_label: "Save links to file",
                                set_halign: gtk::Align::End,
                                connect_clicked => AppInputMsg::SaveLinksToFile,
                            },
                        },

                        gtk::Box {
                            set_orientation: gtk::Orientation::Horizontal,
                            set_spacing: 5,
                            set_margin_all: 5,

                            gtk::Label {
                                set_label: "Status:",
                            },
                            #[name = "status"]
                            gtk::Entry {
                                set_hexpand: true,
                                set_halign: gtk::Align::Fill,
                                set_editable: false,
                                #[watch]
                                set_text: &model.status,
                            }
                        }
                    }
                }
            }
        }
    }

    // Initialize the UI.
    fn init(
        _init: Self::Init,
        _root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // TODO: more robust finding of downloads folder.
        #[cfg(windows)]
        let output_path = std::env::var("USERPROFILE")
            .map(|home| home + r"\Downloads\firefox-links")
            .unwrap_or_default()
            .into();
        #[cfg(not(windows))]
        let output_path = Default::default();

        let background_worker = {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::task::spawn(AppModel::background_work_loop(rx));
            tx
        };

        let wizard_profiles: FactoryVecDeque<RowLabelModel> = FactoryVecDeque::builder()
            .launch(gtk::ListBox::default())
            .detach();

        let sidebar_list_items: FactoryVecDeque<RowLabelModel> = FactoryVecDeque::builder()
            .launch(gtk::ListBox::default())
            .detach();

        let model = AppModel {
            sidebar_indexes: Default::default(),
            sidebar_activated_temp: Default::default(),
            sidebar_queued_activations: Default::default(),
            sidebar_list_items,
            input_path: Default::default(),
            loaded_data: Default::default(),
            tab_groups: Default::default(),
            output_path,
            output_format: actions::FormatInfo::PDF,
            status: Default::default(),
            is_showing_wizard: Default::default(),
            wizard_profiles,
            background_worker,
        };
        let wizard_profile_list = model.wizard_profiles.widget();
        let sidebar_list = model.sidebar_list_items.widget();

        // Insert the macro code generation here
        let widgets = view_output!();

        widgets.preview.buffer().connect_changed({
            let sender = sender.input_sender().clone();
            move |_| sender.emit(AppInputMsg::PreviewChanged)
        });

        ComponentParts { model, widgets }
    }

    // React to user input or changes in UI elements:
    fn update_with_view(
        &mut self,
        widgets: &mut Self::Widgets,
        message: Self::Input,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match message {
            AppInputMsg::WindowShow => {
                #[cfg(feature = "dark-theme")]
                dark_theme::set_for_window(&widgets.window);
            }
            // Mimic `Ctrl+left click` for regular `left click`.
            AppInputMsg::SidebarRowSelected(index) => {
                let queued = self.sidebar_queued_activations;
                self.sidebar_queued_activations = queued.saturating_sub(1);
                if queued == 0 {
                    if self.sidebar_indexes.contains(&index) {
                        // If we activated a row that was already selected then we
                        // want to unselect it:
                        if let Some(row) = widgets.sidebar_list.row_at_index(index) {
                            widgets.sidebar_list.unselect_row(&row);
                        }
                        // Forget it was ever selected:
                        if let Some(ix) =
                            self.sidebar_indexes.iter().position(|&item| item == index)
                        {
                            self.sidebar_indexes.remove(ix);
                        }
                    }
                    self.sidebar_activated_temp = Some(index);
                }
            }
            AppInputMsg::SidebarRowsChanged => {
                let check_to_reselect = self
                    .sidebar_activated_temp
                    .take()
                    .map(|ix| {
                        std::mem::take(&mut self.sidebar_indexes)
                            // Ignore the affected row:
                            .into_iter()
                            .filter(|&item| item != ix)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();

                // Update model/state:
                self.sidebar_indexes.clear();
                self.sidebar_indexes.extend(
                    widgets
                        .sidebar_list
                        .selected_rows()
                        .into_iter()
                        .map(|row| row.index()),
                );

                // Re-select items that was deselected by left click:
                for index in check_to_reselect {
                    if !self.sidebar_indexes.contains(&index) {
                        if let Some(row) = widgets.sidebar_list.row_at_index(index) {
                            self.sidebar_queued_activations += 1;
                            self.sidebar_indexes.push(index);
                            widgets.sidebar_list.select_row(Some(&row));
                        }
                    }
                }

                if self.sidebar_queued_activations == 0 {
                    // Update preview
                    sender
                        .command_sender()
                        .emit(AppCommandMsg::RegeneratePreview);
                }
            }
            AppInputMsg::EditedInputPath => {
                Self::update_path(&mut self.input_path, &widgets.input_path.text());
            }
            AppInputMsg::BrowseInput => {
                let sender = sender.command_sender().clone();
                self.queue_background_work(async move {
                    if let Some(path) = actions::prompt_load_file().await {
                        sender.emit(AppCommandMsg::SetInputPath(path));
                    }
                });
            }
            AppInputMsg::OpenWizard => {
                let mut profiles = self.wizard_profiles.guard();
                profiles.clear();
                for profile in actions::FirefoxProfileInfo::all_profiles() {
                    profiles.push_back(RowLabelModel {
                        name: profile.name().into_owned(),
                        data: Box::new(profile),
                        ..Default::default()
                    });
                }
                self.is_showing_wizard = true;
            }
            AppInputMsg::SelectedWizardProfile(index) => {
                if let Some(profile) = self.wizard_profiles.get(index as usize) {
                    let profile = profile
                        .data
                        .downcast_ref::<actions::FirefoxProfileInfo>()
                        .expect("Failed to downcast profile wizard's list item data");

                    sender.command_sender().emit(AppCommandMsg::SetInputPath(
                        profile.find_sessionstore_file(),
                    ));
                    sender.input_sender().emit(AppInputMsg::LoadNewData);
                }
            }
            AppInputMsg::CloseWizard => {
                self.is_showing_wizard = false;
            }
            AppInputMsg::LoadNewData => {
                let mut data = actions::FileInfo::new(self.input_path.clone());
                self.loaded_data = Some(data.clone());
                widgets.sidebar_list.unselect_all();
                self.status = "Reading input file".to_string();

                let sender = sender.command_sender().clone();
                self.queue_background_work(async move {
                    sender.emit(match data.load_data().await {
                        Ok(()) => AppCommandMsg::UpdateLoadedData(data),
                        Err(e) => AppCommandMsg::SetStatus(format!("Failed to read file: {e}")),
                    });
                });
            }
            AppInputMsg::PreviewChanged => {
                // Hide and then show scrollbar after gtk::TextView was changed
                // to fix scrollbar disappearing on Windows.
                if cfg!(windows) {
                    widgets
                        .preview_scrolled_window
                        .set_policy(gtk::PolicyType::External, gtk::PolicyType::External);

                    sender.oneshot_command(async move {
                        tokio::time::sleep(Duration::from_millis(1)).await;
                        AppCommandMsg::FixPreviewScrollbar
                    });
                }
            }
            AppInputMsg::EditedOutputPath => {
                Self::update_path(&mut self.output_path, &widgets.output_path.text());
            }
            AppInputMsg::BrowseOutput => {
                let sender = sender.command_sender().clone();
                self.queue_background_work(async move {
                    if let Some(path) = actions::prompt_save_file().await {
                        sender.emit(AppCommandMsg::SetOutputPath(path));
                    }
                });
            }
            AppInputMsg::OutputFormatChanged => {
                if let Some(new) = actions::FormatInfo::all()
                    .get(usize::try_from(widgets.output_format.selected()).unwrap())
                    .cloned()
                {
                    self.output_format = new;
                }
            }
            AppInputMsg::CopyLinksToClipboard => {
                let buffer = widgets.preview.buffer();
                let preview = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
                let preview = preview.as_str();

                let display = gdk::Display::default().expect("GTK display not found");
                display.clipboard().set_text(preview);
            }
            AppInputMsg::SaveLinksToFile => {
                if let Some(data) = self.loaded_data.clone() {
                    let save_path = self.output_path.clone();
                    let selected = self.selected_tab_groups(widgets);
                    let output_options = actions::OutputOptions {
                        format: self.output_format,
                        overwrite: widgets.should_overwrite.is_active(),
                        create_folder: widgets.should_create_folder.is_active(),
                    };

                    self.status = "Saving links to file".to_string();

                    let sender = sender.command_sender().clone();
                    self.queue_background_work(async move {
                        sender.emit(AppCommandMsg::SetStatus(
                            match data.save_links(save_path, selected, output_options).await {
                                Ok(()) => "Successfully saved links to a file".to_string(),
                                Err(e) => format!("Failed to save links to file: {e}"),
                            },
                        ));
                    });
                };
            }
        }

        self.update_view(widgets, sender);
    }

    // Handle results of background tasks:
    fn update_cmd_with_view(
        &mut self,
        widgets: &mut Self::Widgets,
        message: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match message {
            AppCommandMsg::SetInputPath(path) => {
                widgets.input_path.set_text(&path.to_string_lossy());
                self.input_path = path;
            }
            AppCommandMsg::SetOutputPath(path) => {
                widgets.output_path.set_text(&path.to_string_lossy());
                self.output_path = path;
            }
            AppCommandMsg::UpdateLoadedData(mut data) => {
                self.loaded_data = Some(data.clone());

                match &data.data {
                    Some(FileData::Compressed { .. }) => {
                        self.status = "Decompressing data".to_string();
                        let sender = sender.command_sender().clone();
                        self.queue_background_work(async move {
                            sender.emit(match data.decompress_data().await {
                                Ok(()) => AppCommandMsg::UpdateLoadedData(data),
                                Err(e) => AppCommandMsg::SetStatus(format!(
                                    "Failed to decompress data: {e}"
                                )),
                            });
                        });
                    }
                    Some(FileData::Uncompressed { .. }) => {
                        self.status = "Parsing session data".to_string();
                        let sender = sender.command_sender().clone();
                        self.queue_background_work(async move {
                            sender.emit(match data.parse_session_data().await {
                                Ok(()) => AppCommandMsg::UpdateLoadedData(data),
                                Err(e) => AppCommandMsg::SetStatus(format!(
                                    "Failed to parse session data: {e}"
                                )),
                            });
                        });
                    }
                    Some(FileData::Parsed { .. }) => {
                        self.status = "Searching for tab groups".to_owned();
                        let sender = sender.command_sender().clone();
                        self.queue_background_work(async move {
                            sender.emit(match data.get_groups_from_session(true).await {
                                Ok(all_groups) => AppCommandMsg::ParsedTabGroups(all_groups),
                                Err(e) => AppCommandMsg::SetStatus(format!(
                                    "Failed to list windows in session: {e}"
                                )),
                            });
                        });
                    }
                    None => unreachable!("Expected to always have data when updating file info"),
                }
            }
            AppCommandMsg::ParsedTabGroups(all_groups) => {
                self.tab_groups = all_groups;
                self.update_sidebar_list();

                sender
                    .command_sender()
                    .emit(AppCommandMsg::RegeneratePreview);
            }
            AppCommandMsg::RegeneratePreview => {
                if let Some(data) = self.loaded_data.clone() {
                    self.status = "Generating preview".to_string();
                    let options = self.selected_tab_groups(widgets);
                    let sender = sender.command_sender().clone();
                    self.queue_background_work(async move {
                        sender.emit(match data.to_text_links(options).await {
                            Ok(text) => AppCommandMsg::SetPreview(text),
                            Err(e) => {
                                AppCommandMsg::SetStatus(format!("Failed to generate preview: {e}"))
                            }
                        });
                    });
                } else {
                    self.status = "".to_owned();
                }
            }
            AppCommandMsg::SetPreview(text) => {
                self.status = "Successfully loaded session data".to_string();
                widgets.preview.buffer().set_text(&text);
            }
            AppCommandMsg::FixPreviewScrollbar => {
                widgets
                    .preview_scrolled_window
                    .set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
            }
            AppCommandMsg::SetStatus(status) => {
                self.status = status;
            }
        }
        self.update_view(widgets, sender);
    }
}

pub const APP_ID: &str = "lej77.firefox-session-ui.gtk4";

pub fn start() {
    #[cfg(windows)]
    {
        if std::env::var_os("GTK_CSD").is_none() {
            // Disable client side decorations, instead let the OS draws window
            // decorations (i.e. the title bar):
            if let Some(warning) = gtk::check_version(4, 12, 0) {
                // Minimize and Maximize buttons are broken on windows, should be fixed
                // by GTK 4.12, see:
                // https://gitlab.gnome.org/GNOME/gtk/-/merge_requests/6367
                eprintln!(
                    "OS window decorations are broken on Windows before GTK 4.12, current version was {}.{}.{}: {warning}",
                    gtk::major_version(), gtk::minor_version(), gtk::micro_version()
                );
            } else {
                std::env::set_var("GTK_CSD", "0");
            }
        }
    }

    gtk::init().unwrap();
    let app = RelmApp::from_app(gtk::Application::new(
        Some(APP_ID),
        ApplicationFlags::NON_UNIQUE,
    ));

    #[cfg(feature = "dark-theme")]
    dark_theme::set_for_app();

    #[cfg(windows)]
    {
        // The default font is really bad so try another one, preferably GTK should use the font
        // specified in settings.ini but that doesn't seem to work, maybe that is only for apps that use
        // libadwaita, try it: https://gtk-rs.org/gtk4-rs/stable/latest/book/libadwaita.html#libadwaita

        // This changes the font size without changing the font:
        relm4::set_global_css_with_priority(
            "* { font-size: 15px; }\n\
            * { font-weight: bold; }",
            gtk::STYLE_PROVIDER_PRIORITY_THEME,
        );

        let display = gdk::Display::default().expect("GTK display not found");
        gtk::Settings::for_display(&display).set_gtk_font_name(Some("Segoe UI"));
    }

    relm4::set_global_css(include_str!("style.css"));
    app.run::<AppModel>(0);
}

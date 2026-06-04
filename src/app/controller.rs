use super::*;

impl Genesis {
    pub(super) fn process_worker_messages(&mut self) {
        while let Ok(message) = self.rx.try_recv() {
            match message {
                WorkerMessage::TerminalLine(line) => {
                    self.terminal.lines.push(line);
                    self.terminal.scroll_to_bottom = true;
                }
                WorkerMessage::TerminalDone => {
                    self.terminal.running = false;
                    self.terminal.scroll_to_bottom = true;
                }
                WorkerMessage::SourceLoaded(Ok(mut loaded)) => {
                    // Set terminal work dir to the game kit root (parent of tags/).
                    self.terminal_work_dir = if let TagSource::LooseFolder { root } = &loaded.source
                    {
                        root.parent().map(|p| p.to_owned())
                    } else {
                        None
                    };
                    // Restore the per-kit terminal-open preference for this game.
                    self.terminal_open = loaded
                        .game
                        .as_deref()
                        .map(|g| self.terminal_open_games.contains(g))
                        .unwrap_or(false);
                    self.names = loaded.names.clone();
                    // Backfill any groups the source's game index lacks (and
                    // cover the case where definitions failed to load on disk).
                    self.names.merge_missing(self.default_names.clone());
                    self.parsed_tags.clear();
                    self.tag_cache_order.clear();
                    self.loading_tags.clear();
                    self.bitmap_previews.clear();
                    self.rmdf_cache.clear();
                    self.rmop_cache.clear();
                    self.color_popup = None;
                    self.function_popup = None;
                    self.selected_key = None;
                    self.open_tabs.clear();
                    self.floating_tabs.clear();
                    if let Some((key, tag)) = loaded.initial_tag.take() {
                        self.selected_key = Some(key.clone());
                        self.open_tabs.push(key.clone());
                        self.remember_tag_use(&key);
                        self.parsed_tags.insert(key, TagDocument::clean(tag));
                    }
                    self.status = format!(
                        "Loaded {} tag(s) from {}",
                        loaded.entries.len(),
                        loaded.label
                    );
                    self.source = Some(loaded);
                    // New entry universe — invalidate any cached search results.
                    self.source_generation = self.source_generation.wrapping_add(1);
                }
                WorkerMessage::SourceLoaded(Err(error)) => {
                    self.status = error;
                }
                WorkerMessage::TagLoaded { key, result } => {
                    self.loading_tags.remove(&key);
                    if !self.open_tabs.iter().any(|tab| tab == &key) {
                        continue;
                    }
                    match result {
                        Ok(tag) => {
                            self.status = "Tag loaded".to_owned();
                            self.remember_tag_use(&key);
                            self.parsed_tags.insert(key, TagDocument::clean(tag));
                            self.trim_tag_memory();
                        }
                        Err(error) => {
                            self.status = error;
                        }
                    }
                }
                WorkerMessage::ExportFinished(result) => {
                    self.status = match result {
                        Ok(message) => message,
                        Err(error) => error,
                    };
                }
                WorkerMessage::AllEntriesScanned(result) => {
                    self.scanning_entries = false;
                    match result {
                        Ok(scanned) => {
                            if let Some(source) = self.source.as_mut() {
                                let n = scanned.len();
                                source.group_tree = crate::source::build_group_tree(&scanned);
                                source.all_entries = scanned;
                                // The full index just landed — cached search
                                // results were built against the partial set.
                                self.source_generation = self.source_generation.wrapping_add(1);
                                self.status = format!("Index complete: {n} tags");
                                // Persist the index in the background so the
                                // next launch can skip the scan entirely.
                                if let (Some(game), TagSource::LooseFolder { root }) =
                                    (source.game.clone(), &source.source)
                                {
                                    let root = root.clone();
                                    let entries = source.all_entries.clone();
                                    thread::spawn(move || {
                                        if let Err(e) =
                                            crate::source::save_entry_index(&game, &root, &entries)
                                        {
                                            eprintln!("index save failed: {e}");
                                        }
                                    });
                                }
                            }
                        }
                        Err(e) => {
                            self.status = format!("Scan failed: {e}");
                        }
                    }
                }
            }
        }
    }

    pub(super) fn begin_load_single(&mut self, ctx: egui::Context) {
        let Some(path) = rfd::FileDialog::new().set_title("Load Tag").pick_file() else {
            return;
        };
        let tx = self.tx.clone();
        let names = self.default_names.clone();
        self.status = format!("Loading {}", path.display());
        thread::spawn(move || {
            let result = load_single_file(path, &names).map_err(|e| e.to_string());
            let _ = tx.send(WorkerMessage::SourceLoaded(result));
            ctx.request_repaint();
        });
    }

    pub(super) fn begin_load_folder(&mut self, ctx: egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Load Folder")
            .pick_folder()
        else {
            return;
        };
        let tx = self.tx.clone();
        let names = self.default_names.clone();
        let definitions_root = locate_definitions_root();
        let folder_info = match resolve_folder_root(&path) {
            Ok(info) => info,
            Err(error) => {
                self.status = error.to_string();
                return;
            }
        };
        self.status = match folder_info.game {
            Some(game) => format!("Indexing {} as {game}", folder_info.scan_root.display()),
            None => format!("Indexing {}", folder_info.scan_root.display()),
        };
        thread::spawn(move || {
            let result = load_folder(path, &names, &definitions_root).map_err(|e| e.to_string());
            let _ = tx.send(WorkerMessage::SourceLoaded(result));
            ctx.request_repaint();
        });
    }

    pub(super) fn begin_load_monolithic(&mut self, ctx: egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Load Monolithic blob_index.dat")
            .add_filter("blob index", &["dat"])
            .pick_file()
        else {
            return;
        };
        let tx = self.tx.clone();
        let names = self.default_names.clone();
        self.status = format!("Opening {}", path.display());
        thread::spawn(move || {
            let result = load_monolithic_blob_index(path, &names).map_err(|e| e.to_string());
            let _ = tx.send(WorkerMessage::SourceLoaded(result));
            ctx.request_repaint();
        });
    }

    /// Trigger a background full recursive scan of a LooseFolder source so
    /// that Groups mode and search work without needing to expand every tree
    /// node first. No-op if already scanning or source is not a LooseFolder.
    pub(super) fn begin_scan_all_entries(&mut self, ctx: egui::Context) {
        if self.scanning_entries {
            return;
        }
        let Some(source) = self.source.as_ref() else {
            return;
        };
        let TagSource::LooseFolder { root } = &source.source else {
            return; // monolithic/single-file already have all entries
        };
        let root = root.clone();
        let names = source.names.clone();
        let tx = self.tx.clone();
        self.scanning_entries = true;
        self.status = "Indexing tags…".to_owned();
        thread::spawn(move || {
            let result = scan_folder_subtree_entries(&root, std::path::Path::new(""), &names)
                .map_err(|e| e.to_string());
            let _ = tx.send(WorkerMessage::AllEntriesScanned(result));
            ctx.request_repaint();
        });
    }

    pub(super) fn begin_terminal_command(&mut self, ctx: egui::Context) {
        let command = self.terminal.input.trim().to_owned();
        if command.is_empty() {
            return;
        }
        self.terminal.input.clear();
        self.spawn_terminal_command(command, ctx);
    }

    /// Run `command` in the editing-kit root, streaming output to the terminal
    /// panel. Shared by the terminal input and the geometry Import button.
    pub(super) fn spawn_terminal_command(&mut self, command: String, ctx: egui::Context) {
        if self.terminal.running {
            self.status = "A command is already running".to_owned();
            return;
        }
        let Some(work_dir) = self.terminal_work_dir.clone() else {
            self.status = "Run requires a loaded editing-kit folder".to_owned();
            return;
        };
        self.terminal_open = true;
        self.terminal.lines.push(format!("> {command}"));
        self.terminal.scroll_to_bottom = true;
        self.terminal.running = true;
        let tx = self.tx.clone();
        thread::spawn(move || {
            #[cfg(target_os = "windows")]
            let mut cmd = {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                let mut c = std::process::Command::new("cmd");
                c.creation_flags(CREATE_NO_WINDOW);
                c.args(["/C", &format!("{command} 2>&1")]);
                c
            };
            #[cfg(not(target_os = "windows"))]
            let mut cmd = {
                let mut c = std::process::Command::new("sh");
                c.args(["-c", &command]);
                c
            };
            cmd.current_dir(&work_dir)
                .stdout(std::process::Stdio::piped())
                .stdin(std::process::Stdio::null());
            match cmd.spawn() {
                Err(e) => {
                    let _ = tx.send(WorkerMessage::TerminalLine(format!("[error] {e}")));
                    let _ = tx.send(WorkerMessage::TerminalDone);
                    ctx.request_repaint();
                }
                Ok(mut child) => {
                    use std::io::BufRead;
                    if let Some(stdout) = child.stdout.take() {
                        let reader = std::io::BufReader::new(stdout);
                        for line in reader.lines() {
                            match line {
                                Ok(l) => {
                                    let _ = tx.send(WorkerMessage::TerminalLine(l));
                                    ctx.request_repaint();
                                }
                                Err(_) => break,
                            }
                        }
                    }
                    let exit = child.wait().ok().and_then(|s| s.code());
                    if let Some(code) = exit {
                        let _ = tx.send(WorkerMessage::TerminalLine(format!("[exit {code}]")));
                    }
                    let _ = tx.send(WorkerMessage::TerminalDone);
                    ctx.request_repaint();
                }
            }
        });
    }

    pub(super) fn select_entry(&mut self, key: String, ctx: egui::Context) {
        if !self.open_tabs.iter().any(|tab| tab == &key) {
            self.open_tabs.push(key.clone());
        }
        self.selected_key = Some(key.clone());
        self.remember_tag_use(&key);
        self.trim_open_tabs();
        self.ensure_tag_loading(key, ctx);
    }

    pub(super) fn ensure_tag_loading(&mut self, key: String, ctx: egui::Context) {
        if self.parsed_tags.contains_key(&key) || self.loading_tags.contains(&key) {
            self.remember_tag_use(&key);
            return;
        }
        let Some(source) = self.source.as_ref() else {
            return;
        };
        // Check both the lazily-loaded entries and the full scan set (all_entries).
        // Flat search results reference all_entries, which may not overlap with entries.
        let Some(entry) = source
            .entries
            .iter()
            .chain(source.all_entries.iter())
            .find(|e| e.key == key)
            .cloned()
        else {
            return;
        };
        let source_kind = source.source.clone();
        let tx = self.tx.clone();
        self.loading_tags.insert(key.clone());
        self.status = format!("Loading {}", entry.display_path);
        thread::spawn(move || {
            let result = read_entry(&source_kind, &entry).map_err(|e| e.to_string());
            let _ = tx.send(WorkerMessage::TagLoaded { key, result });
            ctx.request_repaint();
        });
    }

    pub(super) fn selected_entry(&self) -> Option<&TagEntry> {
        let key = self.selected_key.as_ref()?;
        self.entry_for_key(key)
    }

    pub(super) fn entry_for_key(&self, key: &str) -> Option<&TagEntry> {
        let source = self.source.as_ref()?;
        source
            .entries
            .iter()
            .chain(source.all_entries.iter())
            .find(|entry| entry.key == key)
    }

    pub(super) fn close_tab(&mut self, key: &str) {
        self.open_tabs.retain(|tab| tab != key);
        self.floating_tabs.remove(key);
        self.unload_tag(key);
        if self.selected_key.as_deref() == Some(key) {
            self.selected_key = self.open_tabs.last().cloned();
        }
        self.color_popup = None;
        self.function_popup = None;
    }

    pub(super) fn pop_tab(&mut self, key: &str) {
        self.open_tabs.retain(|tab| tab != key);
        self.floating_tabs.insert(key.to_owned());
        if self.selected_key.as_deref() == Some(key) {
            self.selected_key = self.open_tabs.last().cloned();
        }
        self.color_popup = None;
        self.function_popup = None;
    }

    pub(super) fn dock_tab(&mut self, key: &str) {
        self.floating_tabs.remove(key);
        if !self.open_tabs.iter().any(|tab| tab == key) {
            self.open_tabs.push(key.to_owned());
        }
        self.selected_key = Some(key.to_owned());
        self.color_popup = None;
        self.function_popup = None;
    }

    pub(super) fn handle_floating_tab_drop(&mut self, ctx: &egui::Context) {
        let Some(key) = self.dragging_floating_tab.clone() else {
            return;
        };
        let Some(rack_rect) = self.tab_rack_rect else {
            return;
        };
        let release = ctx.input(|input| {
            input
                .pointer
                .interact_pos()
                .map(|pos| (input.pointer.any_released(), pos))
        });
        let Some((released, pos)) = release else {
            return;
        };
        if !released {
            return;
        }
        if rack_rect.contains(pos) {
            self.dock_tab(&key);
        }
        self.dragging_floating_tab = None;
    }

    pub(super) fn close_all_tabs(&mut self) {
        self.open_tabs.clear();
        self.floating_tabs.clear();
        self.parsed_tags.clear();
        self.loading_tags.clear();
        self.tag_cache_order.clear();
        self.bitmap_previews.clear();
        self.edit_buffers.clear();
        self.selected_key = None;
        self.color_popup = None;
        self.function_popup = None;
    }

    pub(super) fn close_all_tabs_but(&mut self, key: &str) {
        self.open_tabs.retain(|tab| tab == key);
        self.floating_tabs.retain(|tab| tab == key);
        self.parsed_tags.retain(|tab, _| tab == key);
        self.loading_tags.retain(|tab| tab == key);
        self.tag_cache_order.retain(|tab| tab == key);
        self.bitmap_previews.retain(|tab, _| tab == key);
        let edit_prefix = format!("{key}|");
        self.edit_buffers
            .retain(|buffer_key, _| buffer_key.starts_with(&edit_prefix));
        self.selected_key = Some(key.to_owned());
        self.color_popup = None;
        self.function_popup = None;
    }

    pub(super) fn unload_tag(&mut self, key: &str) {
        self.parsed_tags.remove(key);
        self.loading_tags.remove(key);
        self.bitmap_previews.remove(key);
        let edit_prefix = format!("{key}|");
        self.edit_buffers
            .retain(|buffer_key, _| !buffer_key.starts_with(&edit_prefix));
        self.tag_cache_order.retain(|tab| tab != key);
    }

    pub(super) fn remember_tag_use(&mut self, key: &str) {
        self.tag_cache_order.retain(|tab| tab != key);
        self.tag_cache_order.push_back(key.to_owned());
    }

    pub(super) fn trim_open_tabs(&mut self) {
        while self.open_tabs.len() > MAX_OPEN_TABS {
            let removable = self
                .open_tabs
                .iter()
                .position(|tab| Some(tab.as_str()) != self.selected_key.as_deref())
                .unwrap_or(0);
            let key = self.open_tabs.remove(removable);
            self.floating_tabs.remove(&key);
            self.unload_tag(&key);
        }
    }

    pub(super) fn trim_tag_memory(&mut self) {
        let open_tabs = self.open_tabs.iter().cloned().collect::<HashSet<_>>();
        self.bitmap_previews
            .retain(|key, _| open_tabs.contains(key));

        let mut attempts = self.tag_cache_order.len();
        while self.parsed_tags.len() > MAX_PARSED_TAGS && attempts > 0 {
            attempts -= 1;
            let Some(key) = self.tag_cache_order.pop_front() else {
                break;
            };
            if Some(key.as_str()) == self.selected_key.as_deref() {
                self.tag_cache_order.push_back(key);
                continue;
            }
            self.parsed_tags.remove(&key);
            self.bitmap_previews.remove(&key);
        }
    }

    pub(super) fn handle_browser_action(&mut self, action: BrowserAction, ctx: egui::Context) {
        match action {
            BrowserAction::Select(key) => self.select_entry(key, ctx),
            BrowserAction::DumpJson(key) => self.begin_export_json(key, ctx),
            BrowserAction::OpenInExplorer(key) => self.open_entry_in_explorer(&key),
            BrowserAction::DumpLoadedFolderJson(keys) => {
                self.begin_export_loaded_folder_json(keys, ctx)
            }
            BrowserAction::DumpLooseFolderJson { rel_path, label } => {
                self.begin_export_loose_folder_json(rel_path, label, ctx)
            }
            BrowserAction::ExtractRaw(key) => self.begin_extract_raw(key, ctx),
            BrowserAction::ExtractBitmap(key) => self.begin_extract_bitmap(key, ctx),
            BrowserAction::ExtractBitmapFolder(keys) => self.begin_extract_bitmap_folder(keys, ctx),
            BrowserAction::ExtractGeometry(key) => self.begin_extract_geometry(key, ctx),
            BrowserAction::ExtractImportInfo(key) => self.begin_extract_import_info(key, ctx),
            BrowserAction::ExtractAnimation(key) => self.begin_extract_animation(key, ctx),
        }
    }

    pub(super) fn open_entry_in_explorer(&mut self, key: &str) {
        let Some(entry) = self.entry_for_key(key).cloned() else {
            self.status = "Tag is no longer in the browser".to_owned();
            return;
        };
        let Some(source) = self.source.as_ref().map(|source| &source.source) else {
            self.status = "No source loaded".to_owned();
            return;
        };
        let path = match (&entry.location, source) {
            (TagEntryLocation::LooseFile(path), _) => path.clone(),
            (_, TagSource::SingleFile { path }) => path.clone(),
            (TagEntryLocation::Monolithic { .. }, TagSource::MonolithicCache { root, .. }) => {
                root.join("blob_index.dat")
            }
            (TagEntryLocation::Monolithic { .. }, _) => {
                self.status = "Monolithic tag has no loose file to show".to_owned();
                return;
            }
        };
        #[cfg(windows)]
        {
            let arg = format!("/select,{}", path.display());
            match Command::new("explorer.exe").arg(arg).spawn() {
                Ok(_) => self.status = format!("Opened {}", path.display()),
                Err(error) => self.status = format!("Could not open File Explorer: {error}"),
            }
        }
        #[cfg(not(windows))]
        {
            let _ = path;
            self.status = "Open with File Explorer is only available on Windows".to_owned();
        }
    }

    pub(super) fn begin_export_json(&mut self, key: String, ctx: egui::Context) {
        let Some((source, entry)) = self.export_context(&key) else {
            return;
        };
        let default_name = format!("{}.json", tag_file_stem(&entry));
        let Some(output) = rfd::FileDialog::new()
            .set_title("Dump Tag JSON")
            .set_file_name(&default_name)
            .save_file()
        else {
            return;
        };
        self.status = format!("Dumping JSON for {}", entry.display_path);
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = export_tag_json(&source, &entry, &output).map_err(|e| e.to_string());
            let _ = tx.send(WorkerMessage::ExportFinished(result));
            ctx.request_repaint();
        });
    }

    pub(super) fn begin_export_loaded_folder_json(
        &mut self,
        keys: Vec<String>,
        ctx: egui::Context,
    ) {
        let Some(source_data) = self.source.as_ref() else {
            return;
        };
        let entries = keys
            .iter()
            .filter_map(|key| source_data.entries.iter().find(|entry| entry.key == *key))
            .cloned()
            .collect::<Vec<_>>();
        if entries.is_empty() {
            self.status = "No loaded tags found in folder".to_owned();
            return;
        }
        let source = source_data.source.clone();
        let Some(output) = rfd::FileDialog::new()
            .set_title("Dump Folder JSON")
            .pick_folder()
        else {
            return;
        };
        self.status = format!("Dumping {} loaded tag(s) to JSON", entries.len());
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result =
                export_tag_json_entries(&source, &entries, &output).map_err(|e| e.to_string());
            let _ = tx.send(WorkerMessage::ExportFinished(result));
            ctx.request_repaint();
        });
    }

    pub(super) fn begin_export_loose_folder_json(
        &mut self,
        rel_path: PathBuf,
        label: String,
        ctx: egui::Context,
    ) {
        let Some(source_data) = self.source.as_ref() else {
            return;
        };
        let TagSource::LooseFolder { root } = &source_data.source else {
            return;
        };
        let root = root.clone();
        let names = source_data.names.clone();
        let Some(output) = rfd::FileDialog::new()
            .set_title("Dump Folder JSON")
            .pick_folder()
        else {
            return;
        };
        self.status = format!("Dumping JSON for folder {label}");
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = export_loose_folder_json(&root, &rel_path, &names, &output)
                .map_err(|e| e.to_string());
            let _ = tx.send(WorkerMessage::ExportFinished(result));
            ctx.request_repaint();
        });
    }

    pub(super) fn begin_extract_raw(&mut self, key: String, ctx: egui::Context) {
        let Some((source, entry)) = self.export_context(&key) else {
            return;
        };
        let Some(output) = rfd::FileDialog::new()
            .set_title("Extract Raw Tag")
            .set_file_name(tag_file_name(&entry).as_str())
            .save_file()
        else {
            return;
        };
        self.status = format!("Extracting raw tag {}", entry.display_path);
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = extract_raw_tag(&source, &entry, &output).map_err(|e| e.to_string());
            let _ = tx.send(WorkerMessage::ExportFinished(result));
            ctx.request_repaint();
        });
    }

    pub(super) fn begin_extract_bitmap(&mut self, key: String, ctx: egui::Context) {
        let Some((source, entry)) = self.export_context(&key) else {
            return;
        };
        let Some(output) = rfd::FileDialog::new()
            .set_title("Extract Bitmap Images")
            .pick_folder()
        else {
            return;
        };
        self.status = format!("Extracting bitmap {}", entry.display_path);
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = extract_bitmap_images(&source, &entry, &output).map_err(|e| e.to_string());
            let _ = tx.send(WorkerMessage::ExportFinished(result));
            ctx.request_repaint();
        });
    }

    pub(super) fn begin_extract_bitmap_folder(&mut self, keys: Vec<String>, ctx: egui::Context) {
        let Some(source_data) = self.source.as_ref() else {
            return;
        };
        let entries = keys
            .iter()
            .filter_map(|key| source_data.entries.iter().find(|entry| entry.key == *key))
            .cloned()
            .collect::<Vec<_>>();
        if entries.is_empty() {
            self.status = "No bitmap tags found in folder".to_owned();
            return;
        }
        let source = source_data.source.clone();
        let Some(output) = rfd::FileDialog::new()
            .set_title("Extract All Bitmaps")
            .pick_folder()
        else {
            return;
        };
        self.status = format!("Extracting {} bitmap tag(s)", entries.len());
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result =
                extract_bitmap_entries(&source, &entries, &output).map_err(|e| e.to_string());
            let _ = tx.send(WorkerMessage::ExportFinished(result));
            ctx.request_repaint();
        });
    }

    pub(super) fn begin_extract_geometry(&mut self, key: String, ctx: egui::Context) {
        let Some((source, entry)) = self.export_context(&key) else {
            return;
        };
        let Some(output) = rfd::FileDialog::new()
            .set_title("Extract Geometry")
            .pick_folder()
        else {
            return;
        };
        self.status = format!("Extracting geometry from {}", entry.display_path);
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result =
                extract_geometry_for_entry(&source, &entry, &output).map_err(|e| e.to_string());
            let _ = tx.send(WorkerMessage::ExportFinished(result));
            ctx.request_repaint();
        });
    }

    pub(super) fn begin_extract_import_info(&mut self, key: String, ctx: egui::Context) {
        let Some((source, entry)) = self.export_context(&key) else {
            return;
        };
        let Some(output) = rfd::FileDialog::new()
            .set_title("Extract Import Info")
            .pick_folder()
        else {
            return;
        };
        self.status = format!("Extracting import info from {}", entry.display_path);
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = run_shell_extraction(&source, &entry, "extract-import-info", &output)
                .map_err(|e| e.to_string());
            let _ = tx.send(WorkerMessage::ExportFinished(result));
            ctx.request_repaint();
        });
    }

    pub(super) fn begin_extract_animation(&mut self, key: String, ctx: egui::Context) {
        let Some((source, entry)) = self.export_context(&key) else {
            return;
        };
        let Some(output) = rfd::FileDialog::new()
            .set_title("Extract Animations")
            .pick_folder()
        else {
            return;
        };
        self.status = format!("Extracting animations from {}", entry.display_path);
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = run_shell_extraction(&source, &entry, "extract-animation", &output)
                .map_err(|e| e.to_string());
            let _ = tx.send(WorkerMessage::ExportFinished(result));
            ctx.request_repaint();
        });
    }

    pub(super) fn export_context(&self, key: &str) -> Option<(TagSource, TagEntry)> {
        let source = self.source.as_ref()?.source.clone();
        let entry = self.entry_for_key(key)?.clone();
        Some((source, entry))
    }

    pub(super) fn save_current_tag(&mut self) {
        let Some(key) = self.selected_key.clone() else {
            self.status = "No tag selected".to_owned();
            return;
        };
        let Some(entry) = self.entry_for_key(&key).cloned() else {
            self.status = "Selected tag is no longer in the source".to_owned();
            return;
        };
        let Some(doc) = self.parsed_tags.get(&key) else {
            self.status = "Load the selected tag before saving".to_owned();
            return;
        };
        if doc.tag.endian != Endian::Le {
            self.status = "Only little-endian loose tags can be saved".to_owned();
            return;
        }
        let TagEntryLocation::LooseFile(path) = &entry.location else {
            self.status = "Monolithic cache tags are read-only".to_owned();
            return;
        };
        match doc.tag.write(path) {
            Ok(()) => {
                if let Some(doc) = self.parsed_tags.get_mut(&key) {
                    doc.dirty = false;
                }
                self.status = format!("Saved {}", path.display());
            }
            Err(error) => self.status = format!("Save failed: {error}"),
        }
    }

    pub(super) fn current_prefs(&self) -> GuiPrefs {
        GuiPrefs {
            browser_mode: self.browser_mode,
            show_browser_prefixes: self.show_browser_prefixes,
            expert_mode: self.expert_mode,
            dark_mode: self.dark_mode,
            blender_path: self.blender_path.clone(),
        }
    }

    pub(super) fn editing_kit_root(&self) -> Option<PathBuf> {
        let TagSource::LooseFolder { root } = &self.source.as_ref()?.source else {
            return None;
        };
        if root
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("tags"))
        {
            return root.parent().map(Path::to_path_buf);
        }
        Some(root.clone())
    }

    pub(super) fn kit_tool_path(&self, executable_name: &str) -> Option<PathBuf> {
        Some(self.editing_kit_root()?.join(executable_name))
    }

    pub(super) fn launch_sapien(&mut self) {
        self.launch_kit_tool("Sapien", "sapien.exe");
    }

    /// The tag_test executable name for the loaded game. Each editing kit ships
    /// its own renamed build (e.g. H3EK is `halo3_tag_test.exe`); fall back to
    /// the generic name when the game is unknown.
    pub(super) fn tag_test_executable(&self) -> &'static str {
        match self.source.as_ref().and_then(|s| s.game.as_deref()) {
            Some("halo3_mcc") => "halo3_tag_test.exe",
            Some("halo3odst_mcc") => "atlas_tag_test.exe",
            Some("haloreach_mcc") => "reach_tag_test.exe",
            Some("halo4_mcc") => "halo4_tag_test.exe",
            _ => "tag_test.exe",
        }
    }

    pub(super) fn launch_tag_test(&mut self) {
        self.launch_kit_tool("tag_test", self.tag_test_executable());
    }

    pub(super) fn launch_blender(&mut self) {
        let Some(path) = self.blender_path.clone() else {
            self.settings_open = true;
            self.status = "Set the Blender path in File > Settings first".to_owned();
            return;
        };
        if !path.is_file() {
            self.status = format!("Blender executable not found: {}", path.display());
            self.settings_open = true;
            return;
        }
        self.spawn_tool("Blender", &path, path.parent().map(Path::to_path_buf));
    }

    pub(super) fn choose_blender_path(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title("Select Blender Executable");
        if let Some(path) = self.blender_path.as_ref().and_then(|path| path.parent()) {
            dialog = dialog.set_directory(path);
        }
        #[cfg(target_os = "windows")]
        {
            dialog = dialog.add_filter("Executable", &["exe"]);
        }
        if let Some(path) = dialog.pick_file() {
            self.blender_path = Some(path.clone());
            self.blender_path_input = path.display().to_string();
            self.status = format!("Blender path set to {}", path.display());
        }
    }

    fn launch_kit_tool(&mut self, label: &str, executable_name: &str) {
        let Some(path) = self.kit_tool_path(executable_name) else {
            self.status = format!("{label} requires a loaded editing-kit folder");
            return;
        };
        if !path.is_file() {
            self.status = format!("{label} executable not found: {}", path.display());
            return;
        }
        self.spawn_tool(label, &path, self.editing_kit_root());
    }

    fn spawn_tool(&mut self, label: &str, path: &Path, work_dir: Option<PathBuf>) {
        let mut command = Command::new(path);
        if let Some(work_dir) = work_dir {
            command.current_dir(work_dir);
        }
        match command.spawn() {
            Ok(_) => self.status = format!("Launched {label}"),
            Err(error) => self.status = format!("Could not launch {label}: {error}"),
        }
    }

    /// Record the current terminal-open state against the loaded game so it
    /// is restored next time that editing kit is opened.
    pub(super) fn remember_terminal_open_for_game(&mut self) {
        let Some(game) = self.source.as_ref().and_then(|s| s.game.clone()) else {
            return;
        };
        if self.terminal_open {
            self.terminal_open_games.insert(game);
        } else {
            self.terminal_open_games.remove(&game);
        }
    }

    /// Resolve a pending "Open referenced tag" request against the loose-folder
    /// tags root and open it in a new tab (creating a transient entry if the
    /// target isn't in the current index).
    pub(super) fn process_pending_open(&mut self, ctx: &egui::Context) {
        let Some(req) = self.pending_open.take() else {
            return;
        };
        let root = match self.source.as_ref().map(|s| &s.source) {
            Some(TagSource::LooseFolder { root }) => root.clone(),
            _ => {
                self.status = "Open requires a loose-folder source".to_owned();
                return;
            }
        };
        // Resolve the file extension from the definitions name index first
        // (covers every group, e.g. collision_model/physics_model), falling
        // back to the built-in table.
        let ext = self
            .names
            .name_for(req.group_tag)
            .or_else(|| blam_tags::paths::group_tag_to_extension(req.group_tag))
            .unwrap_or("");
        // Normalize: tolerate forward slashes and a path that already carries
        // its extension (e.g. a shader bitmap ref), so we don't double-append.
        let mut rel = req.rel_path.replace('/', "\\");
        if !ext.is_empty() {
            if let Some(stripped) = rel
                .strip_suffix(&format!(".{ext}"))
                .or_else(|| rel.strip_suffix(&format!(".{}", ext.to_ascii_uppercase())))
            {
                rel = stripped.to_owned();
            }
        }
        let abs = blam_tags::paths::resolve_tag_path(&root, &rel, ext);
        if !abs.exists() {
            self.status = format!(
                "Referenced tag not found: {} (group {})",
                abs.display(),
                blam_tags::format_group_tag(req.group_tag)
            );
            return;
        }
        let key = format!("file:{}", abs.display());
        // Ensure an entry exists so ensure_tag_loading can resolve it.
        if self.entry_for_key(&key).is_none() {
            let group_name = self.names.name_for(req.group_tag).map(str::to_owned);
            let display_path = if ext.is_empty() {
                req.rel_path.replace('\\', "/")
            } else {
                format!("{}.{ext}", req.rel_path.replace('\\', "/"))
            };
            let entry = TagEntry {
                key: key.clone(),
                display_path,
                group_tag: req.group_tag,
                group_name,
                location: TagEntryLocation::LooseFile(abs),
            };
            if let Some(source) = self.source.as_mut() {
                source.entries.push(entry);
            }
        }
        self.select_entry(key, ctx.clone());
    }

    /// Run a geometry Import request (`tool render/collision/physics/...`)
    /// streamed to the terminal panel.
    pub(super) fn process_pending_tool_import(&mut self, ctx: &egui::Context) {
        let Some(req) = self.pending_tool_import.take() else {
            return;
        };
        if self.editing_kit_root().is_none() {
            self.status = "Import requires a loaded editing-kit folder".to_owned();
            return;
        }
        let command = format!("tool {} \"{}\"", req.verb, req.source_dir);
        self.spawn_terminal_command(command, ctx.clone());
    }

    /// Render the block delete/delete-all confirmation modal (if pending) and
    /// apply the op on confirm.
    pub(super) fn handle_block_confirm(&mut self, ctx: &egui::Context) {
        let Some(confirm) = self.block_confirm.as_ref() else {
            return;
        };
        let message = confirm.message.clone();
        let confirm_label = confirm.confirm_label.clone();
        let mut do_apply = false;
        let mut do_cancel = false;
        egui::Window::new("Confirm")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(RichText::new(message).color(text_dark()));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(&confirm_label)
                                    .color(Color32::from_rgb(230, 230, 228)),
                            )
                            .fill(Color32::from_rgb(150, 48, 40))
                            .min_size(Vec2::new(80.0, 24.0)),
                        )
                        .clicked()
                    {
                        do_apply = true;
                    }
                    if ui
                        .add(egui::Button::new("Cancel").min_size(Vec2::new(80.0, 24.0)))
                        .clicked()
                    {
                        do_cancel = true;
                    }
                });
            });
        if do_apply {
            if let Some(confirm) = self.block_confirm.take() {
                if let Some(doc) = self.parsed_tags.get_mut(&confirm.tag_key) {
                    let op = BlockOp {
                        path: confirm.path,
                        kind: confirm.kind,
                    };
                    if let Some(status) = apply_block_ops(&mut doc.tag, vec![op], &mut doc.dirty) {
                        self.status = status;
                    }
                }
            }
        } else if do_cancel {
            self.block_confirm = None;
        }
    }

    pub(super) fn persist_prefs_if_changed(&mut self) {
        let prefs = self.current_prefs();
        if prefs == self.saved_prefs && self.terminal_open_games == self.saved_terminal_open_games {
            return;
        }
        match save_gui_prefs(&prefs, &self.terminal_open_games) {
            Ok(()) => {
                self.saved_prefs = prefs;
                self.saved_terminal_open_games = self.terminal_open_games.clone();
            }
            Err(error) => self.status = error,
        }
    }

    pub(super) fn draw_floating_tabs(&mut self, ctx: &egui::Context) {
        let keys = self.floating_tabs.iter().cloned().collect::<Vec<_>>();
        let mut closed = Vec::new();
        for key in keys {
            let Some(entry) = self.entry_for_key(&key).cloned() else {
                closed.push(key);
                continue;
            };
            let Some(mut doc) = self.parsed_tags.remove(&key) else {
                continue;
            };
            let mut open = true;
            let mut dock_requested = false;
            let mut edit_status = None;
            let window_response = egui::Window::new(tag_tab_label(&entry))
                .id(egui::Id::new(("floating_tag", key.clone())))
                .resizable(true)
                .default_width(860.0)
                .default_height(640.0)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        let dock = ui
                            .add(
                                egui::Label::new(RichText::new("dock").color(text_dark()).strong())
                                    .sense(Sense::click_and_drag()),
                            )
                            .on_hover_text("Click to dock, or drag onto the tab rack");
                        if dock.clicked() {
                            dock_requested = true;
                        }
                        if dock.drag_started() || dock.dragged() {
                            self.dragging_floating_tab = Some(key.clone());
                        }
                        ui.label(
                            RichText::new("drag to tab rack")
                                .color(subtle_dark())
                                .small(),
                        );
                    });
                    ui.separator();
                    draw_entry_header(ui, &entry, &self.names);
                    let supports_field_search = supports_field_search(&entry);
                    if supports_field_search {
                        self.draw_field_search_bar(ui, &key);
                    }
                    let mut pending = Vec::new();
                    let mut block_ops = Vec::new();
                    let mut shader_ops = Vec::new();
                    let mut shader_param_ops = Vec::new();
                    let field_filter = compute_pending_field_filter(
                        &doc.tag,
                        supports_field_search,
                        &key,
                        &self.field_search,
                        &mut self.field_search_applied,
                    );
                    let mut color_request = None;
                    let mut block_clip_request = None;
                    let mut edit_context = FieldEditContext {
                        view_scope: "floating",
                        tag_key: &key,
                        group_tag: entry.group_tag,
                        tags_root: self
                            .source
                            .as_ref()
                            .and_then(|source| match &source.source {
                                TagSource::LooseFolder { root } => Some(root.as_path()),
                                _ => None,
                            }),
                        editable: is_editable_tag(&entry, &doc.tag),
                        buffers: &mut self.edit_buffers,
                        pending: &mut pending,
                        block_ops: &mut block_ops,
                        block_confirm: &mut self.block_confirm,
                        open_request: &mut self.pending_open,
                        tool_import: &mut self.pending_tool_import,
                        shader_ops: &mut shader_ops,
                        shader_param_ops: &mut shader_param_ops,
                        color_request: &mut color_request,
                        block_clipboard: self.block_clipboard.as_ref(),
                        block_clip_request: &mut block_clip_request,
                        field_filter: field_filter.as_ref(),
                    };
                    draw_tag(
                        ui,
                        &doc.tag,
                        &entry,
                        &self.names,
                        self.source.as_ref().map(|source| &source.source),
                        &mut self.rmdf_cache,
                        &mut self.rmop_cache,
                        &mut self.color_popup,
                        &mut self.function_popup,
                        self.expert_mode,
                        &mut edit_context,
                    );
                    edit_status = apply_pending_edits(&mut doc.tag, pending, &mut doc.dirty);
                    if let Some(status) = apply_block_ops(&mut doc.tag, block_ops, &mut doc.dirty) {
                        edit_status = Some(status);
                    }
                    if let Some(status) = apply_shader_ops(&mut doc.tag, shader_ops, &mut doc.dirty)
                    {
                        edit_status = Some(status);
                    }
                    if let Some(status) =
                        apply_shader_param_ops(&mut doc.tag, shader_param_ops, &mut doc.dirty)
                    {
                        edit_status = Some(status);
                    }
                    // A color swatch was clicked: open the shared picker.
                    if let Some(popup) = color_request {
                        self.color_popup = Some(popup);
                    }
                    // Element(s) were copied: stash them on the clipboard.
                    if let Some(clip) = block_clip_request {
                        edit_status = Some(format!(
                            "Copied {} '{}' element(s)",
                            clip.elements.len(),
                            clip.label
                        ));
                        self.block_clipboard = Some(clip);
                    }
                });
            if let Some(inner) = &window_response {
                let pointer_down_over_window = ctx.input(|input| {
                    input.pointer.primary_down()
                        && input
                            .pointer
                            .interact_pos()
                            .is_some_and(|pos| inner.response.rect.contains(pos))
                });
                if inner.response.drag_started()
                    || inner.response.dragged()
                    || pointer_down_over_window
                {
                    self.dragging_floating_tab = Some(key.clone());
                }
            }
            if !open {
                closed.push(key.clone());
            }
            self.parsed_tags.insert(key.clone(), doc);
            if open && dock_requested {
                self.dock_tab(&key);
                self.dragging_floating_tab = None;
            }
            if let Some(status) = edit_status {
                self.status = status;
            }
        }
        for key in closed {
            self.floating_tabs.remove(&key);
        }
    }
}

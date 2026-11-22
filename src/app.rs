//! egui 기반 UI.

use std::net::IpAddr;
use std::time::{Duration, Instant};

use eframe::egui;
use egui_extras::{Column, TableBuilder};

use crate::elevate;
use crate::persist::{self, PersistentRoute};
use crate::route::{self, Interface, Route, RouteKey, RouteSpec};
use crate::selection::Selection;

pub const APP_TITLE: &str = "RouteWorker";
/// 창 아이콘과 "정보" 대화 상자에 쓰는 앱 아이콘 (256px).
pub const APP_ICON_PNG: &[u8] = include_bytes!("../assets/routeworker-icon-256.png");
/// "정보" 대화 상자의 회사 로고 (밝은 배경용 가로형). 색·비율을 바꾸지 말 것 (브랜드 규칙).
const COMPANY_LOGO_PNG: &[u8] = include_bytes!("../assets/dhplab-logo-horizontal-640.png");
const PUBLISHER: &str = "DHPLab Consulting";
const COPYRIGHT: &str = "Copyright (c) 2026 DHPLab Consulting";
const WEBSITE_URL: &str = "https://dhplab.com";
/// UI가 한국어이므로 한국어 제품 페이지로 연결한다 (영어: https://dhplab.com/routeworker/).
const PRODUCT_URL: &str = "https://dhplab.com/ko/routeworker/";
const REPO_URL: &str = "https://github.com/donghyunpark-dhplab/ROUTEWorker";

const AUTO_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
const ROW_HEIGHT: f32 = 24.0;
const COLOR_OK: egui::Color32 = egui::Color32::from_rgb(0x2e, 0x9e, 0x5b);
const COLOR_ERR: egui::Color32 = egui::Color32::from_rgb(0xd6, 0x45, 0x45);
const COLOR_WARN: egui::Color32 = egui::Color32::from_rgb(0xd9, 0x8a, 0x1f);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Active,
    Persistent,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FamilyFilter {
    All,
    V4,
    V6,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortColumn {
    Dest,
    Gateway,
    Interface,
    Metric,
    Kind,
    Persistent,
}

/// 추가/편집 대화 상자의 입력 상태.
struct RouteForm {
    /// 편집 중인 원본 라우트. `None`이면 새 라우트 추가.
    editing: Option<Route>,
    dest: String,
    mask: String,
    gateway: String,
    /// `None`이면 게이트웨이 기준으로 자동 선택.
    if_index: Option<u32>,
    metric: u32,
    persistent: bool,
    error: Option<String>,
}

impl RouteForm {
    fn new() -> Self {
        Self {
            editing: None,
            dest: String::new(),
            mask: String::new(),
            gateway: String::new(),
            if_index: None,
            metric: 1,
            persistent: false,
            error: None,
        }
    }

    fn from_route(route: &Route, persistent: bool) -> Self {
        Self {
            editing: Some(route.clone()),
            dest: route.dest.to_string(),
            mask: match route.dest {
                IpAddr::V4(_) => route.mask_text(),
                IpAddr::V6(_) => route.prefix_len.to_string(),
            },
            gateway: if route.is_on_link() {
                String::new()
            } else {
                route.next_hop.to_string()
            },
            if_index: Some(route.if_index),
            metric: route.metric,
            persistent,
            error: None,
        }
    }

    /// 입력을 검증해 라우트 사양으로 바꾼다.
    fn build(&self, interfaces: &[Interface]) -> Result<RouteSpec, String> {
        let (dest, prefix_len) = route::parse_destination(&self.dest, &self.mask)?;
        let gateway = route::parse_gateway(&self.gateway, dest)?;
        let if_index = match (self.if_index, gateway) {
            (Some(index), _) => index,
            (None, Some(gw)) => route::pick_interface(gw, interfaces).ok_or_else(|| {
                format!(
                    "게이트웨이 {gw}와 같은 서브넷에 있는 인터페이스를 찾지 못했습니다. \
                     인터페이스를 직접 선택하세요."
                )
            })?,
            (None, None) => {
                return Err(
                    "게이트웨이가 없는(on-link) 라우트는 인터페이스를 선택해야 합니다.".to_owned(),
                );
            }
        };
        if self.persistent && !(dest.is_ipv4() && gateway.is_some()) {
            return Err("영구 라우트는 게이트웨이가 있는 IPv4 라우트만 지원합니다.".to_owned());
        }
        Ok(RouteSpec {
            dest,
            prefix_len,
            gateway,
            if_index,
            metric: self.metric,
        })
    }
}

enum Confirm {
    DeleteRoutes(Vec<Route>),
    DeletePersistent(PersistentRoute),
}

/// 테이블 행에서 발생한 동작 (그리는 도중에는 `self`를 바꿀 수 없어 모아 둔다).
enum RowAction {
    Click {
        key: RouteKey,
        ctrl: bool,
        shift: bool,
    },
    ContextClick(RouteKey),
    Edit(Route),
    DeleteSelected,
    CopySelected,
}

pub struct App {
    elevated: bool,
    routes: Vec<Route>,
    interfaces: Vec<Interface>,
    persistent: Vec<PersistentRoute>,
    last_refresh: Instant,
    auto_refresh: bool,

    tab: Tab,
    family: FamilyFilter,
    search: String,
    /// 연결이 끊긴 인터페이스의 라우트도 보여 줄지 (`route print`는 숨긴다).
    show_disconnected: bool,
    sort: SortColumn,
    sort_ascending: bool,
    selected: Selection<RouteKey>,

    form: Option<RouteForm>,
    confirm: Option<Confirm>,
    about_open: bool,
    /// "정보" 대화 상자를 처음 열 때 만든다: (앱 아이콘, 회사 로고).
    about_textures: Option<(egui::TextureHandle, egui::TextureHandle)>,
    /// (성공 여부, 메시지)
    status: Option<(bool, String)>,
}

/// 내장 PNG를 egui 텍스처로 올린다.
fn load_png_texture(ctx: &egui::Context, name: &str, png: &[u8]) -> egui::TextureHandle {
    let icon = eframe::icon_data::from_png_bytes(png).expect("내장 PNG가 올바르지 않음");
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [icon.width as usize, icon.height as usize],
        &icon.rgba,
    );
    ctx.load_texture(name, image, egui::TextureOptions::LINEAR)
}

impl App {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let mut app = Self {
            elevated: elevate::is_elevated(),
            routes: Vec::new(),
            interfaces: Vec::new(),
            persistent: Vec::new(),
            last_refresh: Instant::now(),
            auto_refresh: true,
            tab: Tab::Active,
            family: FamilyFilter::V4,
            search: String::new(),
            show_disconnected: false,
            sort: SortColumn::Dest,
            sort_ascending: true,
            selected: Selection::default(),
            form: None,
            confirm: None,
            about_open: false,
            about_textures: None,
            status: None,
        };
        app.refresh();
        app
    }

    /// 라우트/인터페이스/영구 라우트를 모두 다시 읽는다. 실패하면 상태 표시줄에 알린다.
    fn refresh(&mut self) {
        self.last_refresh = Instant::now();
        let mut errors = Vec::new();
        match route::list_interfaces() {
            Ok(v) => self.interfaces = v,
            Err(e) => errors.push(e),
        }
        match route::list_routes() {
            Ok(v) => self.routes = v,
            Err(e) => errors.push(e),
        }
        match persist::list() {
            Ok(v) => self.persistent = v,
            Err(e) => errors.push(e),
        }
        if !errors.is_empty() {
            self.status = Some((false, errors.join(" / ")));
        }
    }

    fn interface(&self, index: u32) -> Option<&Interface> {
        self.interfaces.iter().find(|i| i.index == index)
    }

    fn is_connected(&self, index: u32) -> bool {
        self.interface(index).is_none_or(|i| i.connected)
    }

    fn interface_name(&self, index: u32) -> String {
        self.interface(index)
            .map_or_else(|| format!("#{index}"), |i| i.alias.clone())
    }

    /// `route print`에 보이는 값 = 라우트 메트릭 + 인터페이스 메트릭.
    fn effective_metric(&self, r: &Route) -> u32 {
        let if_metric = self
            .interface(r.if_index)
            .and_then(|i| i.metric_for(r.is_ipv4()));
        r.metric.saturating_add(if_metric.unwrap_or(0))
    }

    fn persistent_entry(&self, r: &Route) -> Option<&PersistentRoute> {
        let (IpAddr::V4(dest), IpAddr::V4(gw)) = (r.dest, r.next_hop) else {
            return None;
        };
        let mask = route::prefix_to_mask_v4(r.prefix_len);
        self.persistent.iter().find(|p| p.matches(dest, mask, gw))
    }

    /// 선택된 라우트를 화면에 보이는 순서대로 돌려준다.
    /// 필터에 가려 보이지 않는 라우트는 포함하지 않는다 (보이지 않는 것을 지우지 않도록).
    fn selected_routes(&self) -> Vec<Route> {
        if self.tab != Tab::Active || self.selected.is_empty() {
            return Vec::new();
        }
        self.visible_routes()
            .into_iter()
            .map(|i| &self.routes[i])
            .filter(|r| self.selected.contains(&r.key()))
            .cloned()
            .collect()
    }

    fn confirm_delete_selected(&mut self) {
        let routes = self.selected_routes();
        if !routes.is_empty() {
            self.confirm = Some(Confirm::DeleteRoutes(routes));
        }
    }

    fn open_edit(&mut self, r: &Route) {
        let persistent = self.persistent_entry(r).is_some();
        self.form = Some(RouteForm::from_route(r, persistent));
    }

    /// 확인 대화 상자에 쓰는 한 줄 요약.
    fn route_summary(&self, r: &Route) -> String {
        format!(
            "{}  →  {}  ·  {}",
            r.dest_text(),
            if r.is_on_link() {
                "On-link".to_owned()
            } else {
                r.next_hop.to_string()
            },
            self.interface_name(r.if_index)
        )
    }

    /// 사용자가 같은 일을 명령줄로 할 때 쓸 수 있는 명령.
    fn command_text(&self, r: &Route) -> String {
        match (r.dest, r.next_hop) {
            (IpAddr::V4(dest), IpAddr::V4(gw)) => format!(
                "route ADD {dest} MASK {} {gw} METRIC {} IF {}",
                r.mask_text(),
                r.metric.max(1),
                r.if_index
            ),
            _ => format!(
                "netsh interface ipv6 add route prefix={} interface={} nexthop={} metric={}",
                r.dest_text(),
                r.if_index,
                r.next_hop,
                r.metric
            ),
        }
    }

    // -----------------------------------------------------------------------
    // 변경 작업
    // -----------------------------------------------------------------------

    /// 추가/편집 폼을 적용한다. 성공 시 상태 메시지를 돌려준다.
    fn apply_form(&mut self, form: &RouteForm) -> Result<String, String> {
        let spec = form.build(&self.interfaces)?;

        let message = match &form.editing {
            None => {
                route::add_route(&spec)?;
                format!("라우트 {}/{} 추가됨", spec.dest, spec.prefix_len)
            }
            Some(old) => {
                if old.key() == spec.key() {
                    if old.metric != spec.metric {
                        route::set_metric(old, spec.metric)?;
                    }
                } else {
                    // 새 라우트를 먼저 만들고 성공했을 때만 기존 것을 지운다.
                    route::add_route(&spec)?;
                    route::delete_route(old).map_err(|e| {
                        format!("새 라우트는 추가했지만 기존 라우트를 지우지 못했습니다: {e}")
                    })?;
                }
                if let Some(entry) = self.persistent_entry(old).cloned() {
                    persist::remove(&entry.value_name)
                        .map_err(|e| format!("활성 라우트는 변경했지만 {e}"))?;
                }
                format!("라우트 {}/{} 변경됨", spec.dest, spec.prefix_len)
            }
        };

        if form.persistent
            && let (IpAddr::V4(dest), Some(IpAddr::V4(gw))) = (spec.dest, spec.gateway)
        {
            persist::add(
                dest,
                route::prefix_to_mask_v4(spec.prefix_len),
                gw,
                spec.metric.max(1),
            )
            .map_err(|e| format!("활성 라우트는 적용했지만 {e}"))?;
            return Ok(format!("{message} (영구 저장)"));
        }
        Ok(message)
    }

    fn delete_route(&mut self, r: &Route) -> Result<String, String> {
        let entry = self.persistent_entry(r).cloned();
        route::delete_route(r)?;
        if let Some(entry) = entry {
            persist::remove(&entry.value_name)
                .map_err(|e| format!("활성 라우트는 삭제했지만 {e}"))?;
            // 같은 영구 항목을 공유하는 다른 라우트가 다시 지우려 하지 않게 한다.
            self.persistent.retain(|p| p.value_name != entry.value_name);
        }
        Ok(format!("라우트 {} 삭제됨", r.dest_text()))
    }

    /// 여러 라우트를 차례로 지운다. 일부가 실패해도 나머지는 계속 진행한다.
    fn delete_routes(&mut self, routes: &[Route]) -> Result<String, String> {
        let mut deleted = 0usize;
        let mut failures = Vec::new();
        let mut last_ok = String::new();
        for r in routes {
            match self.delete_route(r) {
                Ok(msg) => {
                    deleted += 1;
                    last_ok = msg;
                }
                Err(e) => failures.push(format!("{}: {e}", r.dest_text())),
            }
        }
        match (deleted, failures.len()) {
            (1, 0) => Ok(last_ok),
            (n, 0) => Ok(format!("라우트 {n}개 삭제됨")),
            (0, 1) => Err(failures.remove(0)),
            (n, f) => Err(format!("{n}개 삭제, {f}개 실패 — {}", failures[0])),
        }
    }

    fn finish(&mut self, result: Result<String, String>) {
        self.status = Some(match result {
            Ok(msg) => (true, msg),
            Err(msg) => (false, msg),
        });
        self.refresh_keep_status();
    }

    fn refresh_keep_status(&mut self) {
        let status = self.status.take();
        self.refresh();
        if self.status.is_none() {
            self.status = status;
        }
    }

    // -----------------------------------------------------------------------
    // UI
    // -----------------------------------------------------------------------

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("⟳ 새로 고침").on_hover_text("F5").clicked() {
                self.refresh();
                self.status = Some((true, "새로 고침 완료".to_owned()));
            }
            ui.separator();

            let hint = "관리자 권한이 필요합니다";
            if ui
                .add_enabled(self.elevated, egui::Button::new("➕ 추가"))
                .on_disabled_hover_text(hint)
                .clicked()
            {
                self.form = Some(RouteForm::new());
            }
            let selected = self.selected_routes();
            if ui
                .add_enabled(
                    self.elevated && selected.len() == 1,
                    egui::Button::new("✏ 편집"),
                )
                .on_disabled_hover_text(if self.elevated {
                    "라우트를 하나만 선택하세요"
                } else {
                    hint
                })
                .clicked()
                && let Some(r) = selected.first()
            {
                self.open_edit(r);
            }
            let delete_label = if selected.len() > 1 {
                format!("🗑 삭제 ({})", selected.len())
            } else {
                "🗑 삭제".to_owned()
            };
            if ui
                .add_enabled(
                    self.elevated && !selected.is_empty(),
                    egui::Button::new(delete_label),
                )
                .on_hover_text("Delete")
                .on_disabled_hover_text(if self.elevated {
                    "라우트를 선택하세요 (Ctrl/Shift+클릭으로 여러 개)"
                } else {
                    hint
                })
                .clicked()
            {
                self.confirm = Some(Confirm::DeleteRoutes(selected));
            }

            ui.separator();
            ui.selectable_value(&mut self.tab, Tab::Active, "활성 라우트");
            ui.selectable_value(
                &mut self.tab,
                Tab::Persistent,
                format!("영구 라우트 ({})", self.persistent.len()),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("정보").on_hover_text("RouteWorker 정보").clicked() {
                    self.about_open = true;
                }
                ui.separator();
                ui.checkbox(&mut self.auto_refresh, "자동 새로 고침");
            });
        });
        ui.add_space(4.0);

        if self.tab == Tab::Active {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.family, FamilyFilter::V4, "IPv4");
                ui.selectable_value(&mut self.family, FamilyFilter::V6, "IPv6");
                ui.selectable_value(&mut self.family, FamilyFilter::All, "전체");
                ui.separator();
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("🔍 검색 (주소, 인터페이스)")
                        .desired_width(220.0),
                );
                ui.separator();
                ui.checkbox(&mut self.show_disconnected, "연결 끊긴 인터페이스 포함")
                    .on_hover_text(
                        "route print 는 연결이 끊긴 인터페이스의 라우트를 보여 주지 않습니다",
                    );
            });
            ui.add_space(4.0);
        }

        if !self.elevated {
            ui.horizontal(|ui| {
                ui.colored_label(
                    COLOR_WARN,
                    "⚠ 읽기 전용 모드 — 라우트를 추가/편집/삭제하려면 관리자 권한이 필요합니다.",
                );
                if ui.button("🛡 관리자 권한으로 다시 시작").clicked() {
                    match elevate::relaunch_as_admin() {
                        Ok(()) => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
                        Err(e) => self.status = Some((false, e)),
                    }
                }
            });
            ui.add_space(4.0);
        }
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            match &self.status {
                Some((true, msg)) => ui.colored_label(COLOR_OK, format!("✔ {msg}")),
                Some((false, msg)) => ui.colored_label(COLOR_ERR, format!("✖ {msg}")),
                None => ui.weak("준비"),
            };
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let selected = self.selected_routes().len();
                ui.weak(format!(
                    "{}표시 {}개 / 전체 {}개 · 영구 {}개 · {}",
                    if selected > 0 {
                        format!("선택 {selected}개 · ")
                    } else {
                        String::new()
                    },
                    self.visible_routes().len(),
                    self.routes.len(),
                    self.persistent.len(),
                    if self.elevated {
                        "관리자"
                    } else {
                        "일반 사용자"
                    },
                ));
            });
        });
    }

    /// 필터와 정렬을 적용한 라우트 인덱스 목록.
    fn visible_routes(&self) -> Vec<usize> {
        let needle = self.search.trim().to_lowercase();
        let mut indices: Vec<usize> = (0..self.routes.len())
            .filter(|&i| {
                let r = &self.routes[i];
                let family_ok = match self.family {
                    FamilyFilter::All => true,
                    FamilyFilter::V4 => r.is_ipv4(),
                    FamilyFilter::V6 => !r.is_ipv4(),
                };
                family_ok
                    && (self.show_disconnected || self.is_connected(r.if_index))
                    && (needle.is_empty()
                        || r.dest_text().contains(&needle)
                        || r.next_hop.to_string().contains(&needle)
                        || self
                            .interface_name(r.if_index)
                            .to_lowercase()
                            .contains(&needle))
            })
            .collect();

        let dest_key = |r: &Route| (r.dest, r.prefix_len, self.effective_metric(r));
        indices.sort_by(|&a, &b| {
            let (a, b) = (&self.routes[a], &self.routes[b]);
            let ord = match self.sort {
                SortColumn::Dest => std::cmp::Ordering::Equal,
                SortColumn::Gateway => a.next_hop.cmp(&b.next_hop),
                SortColumn::Interface => self
                    .interface_name(a.if_index)
                    .cmp(&self.interface_name(b.if_index)),
                SortColumn::Metric => self.effective_metric(a).cmp(&self.effective_metric(b)),
                SortColumn::Kind => a.kind_text().cmp(&b.kind_text()),
                SortColumn::Persistent => self
                    .persistent_entry(b)
                    .is_some()
                    .cmp(&self.persistent_entry(a).is_some()),
            }
            .then_with(|| dest_key(a).cmp(&dest_key(b)));
            if self.sort_ascending {
                ord
            } else {
                ord.reverse()
            }
        });
        indices
    }

    fn header_cell(&mut self, ui: &mut egui::Ui, label: &str, column: SortColumn) {
        let text = if self.sort == column {
            format!("{label} {}", if self.sort_ascending { "⬆" } else { "⬇" })
        } else {
            label.to_owned()
        };
        if ui
            .add(egui::Button::new(egui::RichText::new(text).strong()).frame(false))
            .clicked()
        {
            if self.sort == column {
                self.sort_ascending = !self.sort_ascending;
            } else {
                self.sort = column;
                self.sort_ascending = true;
            }
        }
    }

    fn active_table(&mut self, ui: &mut egui::Ui) {
        let visible = self.visible_routes();
        let keys: Vec<RouteKey> = visible.iter().map(|&i| self.routes[i].key()).collect();
        // 사라졌거나 필터에 가려진 라우트는 선택에서 뺀다.
        self.selected.retain(|k| keys.contains(k));

        let mut action: Option<RowAction> = None;
        let elevated = self.elevated;
        let modifiers = ui.input(|i| i.modifiers);

        // 행 클릭이 라벨의 텍스트 선택에 가로채이지 않게 한다.
        ui.style_mut().interaction.selectable_labels = false;

        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(230.0).at_least(120.0).clip(true))
            .column(Column::initial(130.0).at_least(80.0).clip(true))
            .column(Column::initial(200.0).at_least(100.0).clip(true))
            .column(Column::initial(200.0).at_least(100.0).clip(true))
            .column(Column::initial(70.0).at_least(50.0))
            .column(Column::initial(70.0).at_least(50.0))
            .column(Column::remainder().at_least(50.0))
            .min_scrolled_height(0.0)
            .header(ROW_HEIGHT + 2.0, |mut header| {
                header.col(|ui| self.header_cell(ui, "대상 네트워크", SortColumn::Dest));
                header.col(|ui| {
                    ui.strong("넷마스크");
                });
                header.col(|ui| self.header_cell(ui, "게이트웨이", SortColumn::Gateway));
                header.col(|ui| self.header_cell(ui, "인터페이스", SortColumn::Interface));
                header.col(|ui| self.header_cell(ui, "메트릭", SortColumn::Metric));
                header.col(|ui| self.header_cell(ui, "유형", SortColumn::Kind));
                header.col(|ui| self.header_cell(ui, "영구", SortColumn::Persistent));
            })
            .body(|body| {
                body.rows(ROW_HEIGHT, visible.len(), |mut row| {
                    let r = &self.routes[visible[row.index()]];
                    let key = &keys[row.index()];
                    let connected = self.is_connected(r.if_index);
                    row.set_selected(self.selected.contains(key));

                    // 연결이 끊긴 인터페이스의 라우트는 행 전체를 흐리게 표시한다.
                    let text = |text: String, monospace: bool| {
                        let mut text = egui::RichText::new(text);
                        if monospace {
                            text = text.monospace();
                        }
                        if !connected {
                            text = text.weak();
                        }
                        text
                    };

                    row.col(|ui| {
                        ui.label(text(r.dest_text(), true));
                    });
                    row.col(|ui| {
                        ui.label(text(r.mask_text(), true));
                    });
                    row.col(|ui| {
                        if r.is_on_link() {
                            // 게이트웨이를 거치지 않는 라우트. 링크 연결 상태와는 무관하다.
                            ui.weak("On-link").on_hover_text(
                                "게이트웨이 없이 인터페이스에서 직접 도달하는 대상입니다",
                            );
                        } else {
                            ui.label(text(r.next_hop.to_string(), true));
                        }
                    });
                    row.col(|ui| {
                        let name = self.interface_name(r.if_index);
                        if connected {
                            ui.label(name);
                        } else {
                            ui.weak(format!("{name} (연결 끊김)"));
                        }
                    });
                    row.col(|ui| {
                        ui.label(text(self.effective_metric(r).to_string(), false))
                            .on_hover_text(format!(
                                "라우트 메트릭 {} + 인터페이스 메트릭 {}",
                                r.metric,
                                self.effective_metric(r) - r.metric
                            ));
                    });
                    row.col(|ui| {
                        ui.label(text(r.kind_text(), false));
                    });
                    row.col(|ui| {
                        if self.persistent_entry(r).is_some() {
                            ui.colored_label(COLOR_OK, "✔ 영구");
                        }
                    });

                    let response = row.response();
                    if response.clicked() {
                        action = Some(RowAction::Click {
                            key: key.clone(),
                            ctrl: modifiers.command,
                            shift: modifiers.shift,
                        });
                    }
                    if response.secondary_clicked() {
                        action = Some(RowAction::ContextClick(key.clone()));
                    }
                    if response.double_clicked() && elevated && !modifiers.any() {
                        action = Some(RowAction::Edit(r.clone()));
                    }
                    response.context_menu(|ui| {
                        // 선택되지 않은 행에서 메뉴를 열면 그 행 하나만 대상이 된다.
                        let count = if self.selected.contains(key) {
                            self.selected.len()
                        } else {
                            1
                        };
                        if ui
                            .add_enabled(elevated && count == 1, egui::Button::new("✏ 편집"))
                            .clicked()
                        {
                            action = Some(RowAction::Edit(r.clone()));
                            ui.close();
                        }
                        let delete_label = if count > 1 {
                            format!("🗑 선택한 {count}개 삭제")
                        } else {
                            "🗑 삭제".to_owned()
                        };
                        if ui
                            .add_enabled(elevated, egui::Button::new(delete_label))
                            .clicked()
                        {
                            action = Some(RowAction::DeleteSelected);
                            ui.close();
                        }
                        ui.separator();
                        let copy_label = if count > 1 {
                            format!("📋 선택한 {count}개를 명령으로 복사")
                        } else {
                            "📋 명령으로 복사".to_owned()
                        };
                        if ui.button(copy_label).clicked() {
                            action = Some(RowAction::CopySelected);
                            ui.close();
                        }
                    });
                });
            });

        match action {
            Some(RowAction::Click { key, ctrl, shift }) => {
                self.selected.click(&key, &keys, ctrl, shift);
            }
            Some(RowAction::ContextClick(key)) => self.selected.context_click(&key),
            Some(RowAction::Edit(r)) => {
                self.selected.select_only(r.key());
                self.open_edit(&r);
            }
            Some(RowAction::DeleteSelected) => self.confirm_delete_selected(),
            Some(RowAction::CopySelected) => {
                let commands: Vec<String> = self
                    .selected_routes()
                    .iter()
                    .map(|r| self.command_text(r))
                    .collect();
                if !commands.is_empty() {
                    ui.ctx().copy_text(commands.join("\r\n"));
                    self.status = Some((
                        true,
                        format!("명령 {}개를 클립보드에 복사했습니다", commands.len()),
                    ));
                }
            }
            None => {}
        }
    }

    fn persistent_table(&mut self, ui: &mut egui::Ui) {
        ui.weak(
            "재부팅 후에도 유지되는 IPv4 라우트입니다 (route -p 와 같은 레지스트리 저장소). \
             여기서 삭제해도 현재 활성 라우트는 그대로 남습니다.",
        );
        ui.add_space(6.0);
        if self.persistent.is_empty() {
            ui.label("저장된 영구 라우트가 없습니다.");
            return;
        }

        let mut delete: Option<PersistentRoute> = None;
        let elevated = self.elevated;
        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(180.0).at_least(100.0))
            .column(Column::initial(160.0).at_least(100.0))
            .column(Column::initial(180.0).at_least(100.0))
            .column(Column::initial(80.0).at_least(50.0))
            .column(Column::initial(90.0).at_least(60.0))
            .column(Column::remainder().at_least(60.0))
            .min_scrolled_height(0.0)
            .header(ROW_HEIGHT + 2.0, |mut header| {
                for title in [
                    "대상 네트워크",
                    "넷마스크",
                    "게이트웨이",
                    "메트릭",
                    "현재 상태",
                    "",
                ] {
                    header.col(|ui| {
                        ui.strong(title);
                    });
                }
            })
            .body(|body| {
                body.rows(ROW_HEIGHT, self.persistent.len(), |mut row| {
                    let p = &self.persistent[row.index()];
                    let active = self.routes.iter().any(|r| {
                        r.dest == IpAddr::V4(p.dest)
                            && route::prefix_to_mask_v4(r.prefix_len) == p.mask
                            && r.next_hop == IpAddr::V4(p.gateway)
                    });
                    row.col(|ui| {
                        ui.monospace(p.dest.to_string());
                    });
                    row.col(|ui| {
                        ui.monospace(p.mask.to_string());
                    });
                    row.col(|ui| {
                        ui.monospace(p.gateway.to_string());
                    });
                    row.col(|ui| {
                        ui.label(if p.metric < 0 {
                            "기본값".to_owned()
                        } else {
                            p.metric.to_string()
                        });
                    });
                    row.col(|ui| {
                        if active {
                            ui.colored_label(COLOR_OK, "활성");
                        } else {
                            ui.weak("비활성");
                        }
                    });
                    row.col(|ui| {
                        if ui
                            .add_enabled(elevated, egui::Button::new("🗑 삭제"))
                            .on_disabled_hover_text("관리자 권한이 필요합니다")
                            .clicked()
                        {
                            delete = Some(p.clone());
                        }
                    });
                });
            });

        if let Some(p) = delete {
            self.confirm = Some(Confirm::DeletePersistent(p));
        }
    }

    fn form_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.form.take() else {
            return;
        };
        let mut submit = false;
        let mut cancel = false;

        let modal = egui::Modal::new(egui::Id::unique("route_form")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.heading(if form.editing.is_some() {
                "라우트 편집"
            } else {
                "라우트 추가"
            });
            ui.add_space(8.0);

            egui::Grid::new("route_form_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("대상 네트워크");
                    ui.add(
                        egui::TextEdit::singleline(&mut form.dest)
                            .hint_text("예: 10.0.0.0 또는 10.0.0.0/8")
                            .desired_width(300.0),
                    );
                    ui.end_row();

                    ui.label("넷마스크 / 프리픽스");
                    ui.add(
                        egui::TextEdit::singleline(&mut form.mask)
                            .hint_text("예: 255.0.0.0 또는 8 (비우면 호스트 라우트)")
                            .desired_width(300.0),
                    );
                    ui.end_row();

                    ui.label("게이트웨이");
                    ui.add(
                        egui::TextEdit::singleline(&mut form.gateway)
                            .hint_text("예: 192.168.0.1 (비우면 on-link)")
                            .desired_width(300.0),
                    );
                    ui.end_row();

                    ui.label("인터페이스");
                    let selected_text = match form.if_index {
                        None => "자동 (게이트웨이 기준)".to_owned(),
                        Some(index) => self
                            .interface(index)
                            .map_or_else(|| format!("#{index}"), Interface::label),
                    };
                    egui::ComboBox::from_id_salt("route_form_iface")
                        .selected_text(selected_text)
                        .width(308.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut form.if_index, None, "자동 (게이트웨이 기준)");
                            for iface in &self.interfaces {
                                ui.selectable_value(
                                    &mut form.if_index,
                                    Some(iface.index),
                                    iface.label(),
                                );
                            }
                        });
                    ui.end_row();

                    ui.label("메트릭");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut form.metric).range(0..=9999));
                        ui.weak("실제 값 = 이 값 + 인터페이스 메트릭");
                    });
                    ui.end_row();

                    ui.label("영구 라우트");
                    ui.checkbox(
                        &mut form.persistent,
                        "재부팅 후에도 유지 (IPv4, 게이트웨이 필수)",
                    );
                    ui.end_row();
                });

            ui.add_space(8.0);
            // 입력이 어떻게 해석되는지 미리 보여 준다.
            match form.build(&self.interfaces) {
                Ok(spec) => {
                    ui.weak(format!(
                        "적용 내용: {}/{} → {} · {}",
                        spec.dest,
                        spec.prefix_len,
                        spec.gateway
                            .map_or_else(|| "on-link".to_owned(), |g| g.to_string()),
                        self.interface_name(spec.if_index),
                    ));
                }
                Err(e) if !form.dest.trim().is_empty() => {
                    ui.colored_label(COLOR_WARN, e);
                }
                Err(_) => {}
            }
            if let Some(error) = &form.error {
                ui.colored_label(COLOR_ERR, format!("✖ {error}"));
            }

            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("취소").clicked() {
                    cancel = true;
                }
                let label = if form.editing.is_some() {
                    "변경 적용"
                } else {
                    "추가"
                };
                if ui.button(label).clicked() {
                    submit = true;
                }
            });
        });

        if cancel || modal.should_close() {
            return;
        }
        if submit {
            match self.apply_form(&form) {
                Ok(msg) => {
                    self.finish(Ok(msg));
                    return;
                }
                Err(e) => {
                    form.error = Some(e);
                    // 일부만 적용됐을 수 있으니 목록은 최신으로 맞춘다.
                    self.refresh_keep_status();
                }
            }
        }
        self.form = Some(form);
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(confirm) = self.confirm.take() else {
            return;
        };
        let mut yes = false;
        let mut no = false;

        let modal = egui::Modal::new(egui::Id::unique("confirm")).show(ctx, |ui| {
            ui.set_width(520.0);
            match &confirm {
                Confirm::DeleteRoutes(routes) => {
                    if let [r] = routes.as_slice() {
                        ui.heading("라우트 삭제");
                        ui.add_space(8.0);
                        ui.label("다음 라우트를 삭제할까요?");
                        ui.monospace(self.route_summary(r));
                    } else {
                        ui.heading(format!("라우트 {}개 삭제", routes.len()));
                        ui.add_space(8.0);
                        ui.label(format!("선택한 라우트 {}개를 삭제할까요?", routes.len()));
                        egui::ScrollArea::vertical()
                            .max_height(220.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                for r in routes {
                                    ui.monospace(self.route_summary(r));
                                }
                            });
                    }

                    let persistent = routes
                        .iter()
                        .filter(|r| self.persistent_entry(r).is_some())
                        .count();
                    if persistent > 0 {
                        ui.add_space(6.0);
                        ui.label(format!("영구 라우트 항목 {persistent}개도 함께 삭제됩니다."));
                    }
                    if routes.iter().any(Route::is_default) {
                        ui.add_space(6.0);
                        ui.colored_label(
                            COLOR_WARN,
                            "⚠ 기본 경로가 포함되어 있습니다. 삭제하면 외부 네트워크 연결이 끊길 수 있습니다.",
                        );
                    }
                    let automatic = routes.iter().filter(|r| !r.is_static()).count();
                    if automatic > 0 {
                        ui.add_space(6.0);
                        ui.colored_label(
                            COLOR_WARN,
                            format!(
                                "⚠ 시스템이 자동으로 만든 라우트 {automatic}개가 포함되어 있습니다. \
                                 삭제해도 다시 생길 수 있습니다."
                            ),
                        );
                    }
                }
                Confirm::DeletePersistent(p) => {
                    ui.heading("영구 라우트 삭제");
                    ui.add_space(8.0);
                    ui.label("다음 영구 라우트 항목을 삭제할까요?");
                    ui.monospace(format!("{} mask {}  →  {}", p.dest, p.mask, p.gateway));
                    ui.weak("현재 활성 라우트는 유지되며, 다음 부팅부터 적용되지 않습니다.");
                }
            }
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("취소").clicked() {
                    no = true;
                }
                if ui.button("삭제").clicked() {
                    yes = true;
                }
            });
        });

        if yes {
            let result = match &confirm {
                Confirm::DeleteRoutes(routes) => self.delete_routes(routes),
                Confirm::DeletePersistent(p) => persist::remove(&p.value_name)
                    .map(|()| format!("영구 라우트 {} 삭제됨", p.dest)),
            };
            self.finish(result);
        } else if !(no || modal.should_close()) {
            self.confirm = Some(confirm);
        }
    }

    fn about_dialog(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }
        let (icon, logo) = self
            .about_textures
            .get_or_insert_with(|| {
                (
                    load_png_texture(ctx, "app_icon", APP_ICON_PNG),
                    load_png_texture(ctx, "company_logo", COMPANY_LOGO_PNG),
                )
            })
            .clone();
        let mut close = false;

        let modal = egui::Modal::new(egui::Id::unique("about")).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.vertical_centered(|ui| {
                ui.add_space(6.0);
                ui.add(egui::Image::new((icon.id(), egui::vec2(72.0, 72.0))));
                ui.add_space(6.0);
                ui.heading(APP_TITLE);
                ui.label(format!("버전 {}", env!("CARGO_PKG_VERSION")));
                ui.add_space(6.0);
                ui.label("Windows 라우팅 테이블을 조회하고 추가·편집·삭제하는 도구");
                ui.add_space(14.0);
                ui.separator();
                // 로고 여백: 마크 높이의 1/4 이상 (브랜드 규칙). 최소 폭 140px.
                ui.add_space(14.0);
                let w = 180.0;
                let [lw, lh] = logo.size();
                ui.add(egui::Image::new((logo.id(), egui::vec2(w, w * lh as f32 / lw as f32))));
                ui.add_space(14.0);
                ui.label(PUBLISHER);
                ui.weak(COPYRIGHT);
                ui.weak("MIT 라이선스");
                ui.add_space(6.0);
                ui.hyperlink_to("RouteWorker 제품 페이지", PRODUCT_URL);
                ui.hyperlink_to("dhplab.com", WEBSITE_URL);
                ui.hyperlink_to("GitHub 저장소", REPO_URL);
            });
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("닫기").clicked() {
                    close = true;
                }
            });
        });
        if close || modal.should_close() {
            self.about_open = false;
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let dialog_open = self.form.is_some() || self.confirm.is_some() || self.about_open;

        if !dialog_open && ui.input(|i| i.key_pressed(egui::Key::F5)) {
            self.refresh();
            self.status = Some((true, "새로 고침 완료".to_owned()));
        }
        // 목록 단축키. 검색창 등에 입력 중일 때는 가로채지 않는다.
        if !dialog_open && self.tab == Tab::Active && !ctx.text_edit_focused() {
            let (delete, select_all, escape) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::Delete),
                    i.modifiers.command && i.key_pressed(egui::Key::A),
                    i.key_pressed(egui::Key::Escape),
                )
            });
            if select_all {
                let keys: Vec<RouteKey> = self
                    .visible_routes()
                    .into_iter()
                    .map(|i| self.routes[i].key())
                    .collect();
                self.selected.select_all(&keys);
            } else if escape {
                self.selected.clear();
            } else if delete && self.elevated {
                self.confirm_delete_selected();
            }
        }
        if self.auto_refresh {
            // 대화 상자가 떠 있는 동안에는 목록을 건드리지 않는다.
            if !dialog_open && self.last_refresh.elapsed() >= AUTO_REFRESH_INTERVAL {
                self.refresh_keep_status();
            }
            ctx.request_repaint_after(AUTO_REFRESH_INTERVAL);
        }

        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.add_space(3.0);
            self.status_bar(ui);
            ui.add_space(3.0);
        });
        egui::CentralPanel::default().show(ui, |ui| match self.tab {
            Tab::Active => self.active_table(ui),
            Tab::Persistent => self.persistent_table(ui),
        });

        self.form_dialog(&ctx);
        self.confirm_dialog(&ctx);
        self.about_dialog(&ctx);
    }
}

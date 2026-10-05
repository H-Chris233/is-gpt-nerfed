use super::*;
use egui::{Color32, CornerRadius, FontId, Frame, Margin, RichText, Sense, Stroke, Vec2};

const BG: Color32 = Color32::from_rgb(244, 247, 250);
const NAV: Color32 = Color32::from_rgb(24, 38, 53);
const INK: Color32 = Color32::from_rgb(30, 48, 65);
const MUTED: Color32 = Color32::from_rgb(107, 124, 140);
const BORDER: Color32 = Color32::from_rgb(226, 233, 240);
const ACCENT: Color32 = Color32::from_rgb(21, 134, 115);
const TEAL_BG: Color32 = Color32::from_rgb(231, 246, 241);
const AMBER: Color32 = Color32::from_rgb(157, 104, 25);
const AMBER_BG: Color32 = Color32::from_rgb(253, 245, 227);
const RED: Color32 = Color32::from_rgb(176, 56, 62);
const RED_BG: Color32 = Color32::from_rgb(253, 235, 236);

fn card<R>(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
    Frame::NONE
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), contents)
                .inner
        })
}
fn text(ui: &mut egui::Ui, content: impl Into<String>) {
    ui.label(RichText::new(content.into()).color(MUTED).size(13.0));
}
fn badge(ui: &mut egui::Ui, label: &str, color: Color32, fill: Color32) {
    Frame::NONE
        .fill(fill)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(9, 4))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(12.0).strong().color(color));
        });
}
fn button(label: impl Into<String>, primary: bool) -> egui::Button<'static> {
    let label = label.into();
    egui::Button::new(RichText::new(label).size(14.0).strong().color(if primary {
        Color32::WHITE
    } else {
        INK
    }))
    .fill(if primary { ACCENT } else { Color32::WHITE })
    .stroke(Stroke::new(1.0, if primary { ACCENT } else { BORDER }))
    .corner_radius(CornerRadius::same(8))
    .min_size(Vec2::new(0.0, 36.0))
}
fn tone(p: &Probe, account: &str) -> (Color32, Color32) {
    if p.account_id != account || p.status == "cancelled" {
        return (MUTED, BG);
    }
    match p.assessment.verdict.as_str() {
        "MATCH" => (ACCENT, TEAL_BG),
        "MISMATCH" | "DOWNGRADED!" => (RED, RED_BG),
        "UNLISTED" | "SUSPICIOUS" | "UNKNOWN" => (AMBER, AMBER_BG),
        _ => (MUTED, BG),
    }
}
fn time_label(value: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| value.into())
}
fn brand(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(38.0), Sense::hover());
    ui.painter().rect_filled(rect, 10.0, ACCENT);
    for (x, height) in [(10.0, 11.0), (17.0, 17.0), (24.0, 23.0)] {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(
                rect.min + egui::vec2(x, 28.0 - height),
                egui::vec2(4.0, height),
            ),
            1.5,
            Color32::WHITE,
        );
    }
}
pub(super) fn render(app: &mut Desktop, root: &mut egui::Ui) {
    let ctx = root.ctx().clone();
    egui::Panel::left("navigation")
        .exact_size(176.0)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            Frame::NONE
                .fill(NAV)
                .inner_margin(Margin::symmetric(18, 24)),
        )
        .show(root, |ui| {
            ui.horizontal(|ui| {
                brand(ui);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Nerfed")
                            .strong()
                            .size(19.0)
                            .color(Color32::WHITE),
                    );
                    ui.label(
                        RichText::new("模型状态观察")
                            .size(11.0)
                            .color(Color32::from_rgb(162, 180, 193)),
                    );
                });
            });
            ui.add_space(36.0);
            for (tab, number, title) in [
                (0, "01", "会话检测"),
                (1, "02", "检测历史"),
                (2, "03", "设置与连接"),
            ] {
                let selected = app.tab == tab;
                let fill = if selected {
                    Color32::from_rgb(43, 68, 80)
                } else {
                    NAV
                };
                let color = if selected {
                    Color32::from_rgb(173, 239, 218)
                } else {
                    Color32::from_rgb(174, 191, 203)
                };
                if ui
                    .add_sized(
                        [140.0, 44.0],
                        egui::Button::new(
                            RichText::new(format!("{number}   {title}"))
                                .size(14.0)
                                .strong()
                                .color(color),
                        )
                        .fill(fill)
                        .stroke(Stroke::NONE)
                        .corner_radius(CornerRadius::same(9)),
                    )
                    .clicked()
                {
                    app.tab = tab;
                }
                ui.add_space(6.0);
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                ui.label(
                    RichText::new(format!("v{} · Windows", env!("CARGO_PKG_VERSION")))
                        .size(11.0)
                        .color(Color32::from_rgb(137, 158, 174)),
                );
                ui.label(
                    RichText::new("分析与记录保存在本机")
                        .size(11.0)
                        .color(Color32::from_rgb(137, 158, 174)),
                );
                ui.add_space(10.0);
                for (label, exit) in [("退出程序", true), ("收起到托盘", false)] {
                    let action = egui::Button::new(
                        RichText::new(label)
                            .size(13.0)
                            .color(Color32::from_rgb(192, 207, 217)),
                    )
                    .fill(Color32::from_rgb(34, 51, 66))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(49, 68, 84)))
                    .corner_radius(CornerRadius::same(8));
                    if ui.add_sized([140.0, 34.0], action).clicked() {
                        if exit {
                            app.request_quit(&ctx);
                        } else {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                            app.hidden = true;
                        }
                    }
                }
                ui.add_space(12.0);
                badge(
                    ui,
                    if app.cfg.automatic {
                        "自动检测已开启"
                    } else {
                        "仅手动主动检测"
                    },
                    Color32::from_rgb(178, 233, 219),
                    Color32::from_rgb(37, 65, 74),
                );
            });
        });
    egui::CentralPanel::default()
        .frame(Frame::NONE.fill(BG).inner_margin(Margin::same(24)))
        .show(root, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    let (title, description) = match app.tab {
                        0 => ("会话检测", "选择一个会话，查看本地记录与模型指纹。"),
                        1 => (
                            "检测历史",
                            "保留每次结果，包括失败、取消和来自其他账户的记录。",
                        ),
                        _ => ("设置与连接", "管理检测频率、通知，以及 Codex 连接状态。"),
                    };
                    ui.label(RichText::new(title).size(25.0).strong().color(INK));
                    text(ui, description);
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(app.task.is_none(), button("刷新状态", false))
                        .clicked()
                    {
                        app.refresh(&ctx, true);
                    }
                });
            });
            ui.add_space(16.0);
            if let Some(task) = &app.task {
                Frame::NONE
                    .fill(TEAL_BG)
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new(&app.status).color(ACCENT));
                            text(ui, format!("{} 秒", task.started.elapsed().as_secs()));
                            if task.kind == "probe" && ui.add(button("取消检测", false)).clicked()
                            {
                                task.cancel.store(true, Ordering::Relaxed);
                                app.status = "正在取消检测…".into();
                            }
                        });
                    });
                ui.add_space(12.0);
            }
            if let Some(error) = &app.error {
                Frame::NONE
                    .fill(RED_BG)
                    .inner_margin(Margin::same(12))
                    .corner_radius(CornerRadius::same(8))
                    .show(ui, |ui| {
                        ui.colored_label(RED, error);
                    });
                ui.add_space(12.0);
            }
            match app.tab {
                0 => app.sessions_ui(ui),
                1 => app.history_ui(ui),
                _ => app.settings_ui(ui),
            }
        });
    if app.confirm_auto {
        egui::Window::new("开启自动检测")
            .collapsible(false)
            .resizable(false)
            .default_width(430.0)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(&ctx, |ui| {
                ui.label(format!(
                    "近期活跃会话按 {} 分钟间隔检测。程序需要保持运行，可收起到托盘。",
                    app.draft.interval_minutes
                ));
                ui.add_space(8.0);
                ui.label(format!(
                    "每次生成 {} 份回答，消耗你的 Codex 额度；超时样本最多补充一次。",
                    app.draft.queries
                ));
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if ui.add(button("开启并保存", true)).clicked() {
                        app.draft.automatic = true;
                        app.confirm_auto = false;
                        app.save(&ctx);
                    }
                    if ui.add(button("保持关闭", false)).clicked() {
                        app.confirm_auto = false;
                    }
                });
            });
    }
}
impl Desktop {
    fn sessions_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let available = ui.available_width();
            for (label, value, detail) in [
                (
                    "用户会话",
                    self.threads.len().to_string(),
                    "已排除内部审批与子任务".to_string(),
                ),
                (
                    "Codex 连接",
                    self.binary
                        .as_ref()
                        .map_or("未连接".into(), |b| b.version.clone()),
                    if self.binary.is_some() {
                        "使用实际发现的可执行文件"
                    } else {
                        "在设置中选择 Codex"
                    }
                    .into(),
                ),
                (
                    "主动检测",
                    if self.cfg.automatic {
                        "自动"
                    } else {
                        "手动"
                    }
                    .into(),
                    if self.cfg.automatic {
                        format!(
                            "{} 分钟间隔 · {} 份回答",
                            self.cfg.interval_minutes, self.cfg.queries
                        )
                    } else {
                        "本地日志仍会自动刷新".into()
                    },
                ),
            ] {
                card(ui, |ui| {
                    ui.set_width(((available - 32.0) / 3.0 - 36.0).max(80.0));
                    text(ui, label);
                    ui.label(RichText::new(value).size(20.0).strong().color(INK));
                    text(ui, detail);
                });
            }
        });
        ui.add_space(16.0);
        let height = ui.available_height().max(240.0);
        let list_width = (ui.available_width() * 0.31).clamp(225.0, 285.0);
        ui.horizontal_top(|ui| {
            card(ui, |ui| {
                ui.set_width(list_width - 36.0);
                ui.set_min_height(height - 36.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("近期会话").strong().size(16.0));
                    text(ui, self.threads.len().to_string());
                });
                ui.add_space(8.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.filter)
                        .hint_text("搜索标题或模型")
                        .desired_width(f32::INFINITY)
                        .margin(Vec2::new(10.0, 8.0)),
                );
                ui.add_space(10.0);
                let filter = self.filter.to_lowercase();
                egui::ScrollArea::vertical()
                    .id_salt("sessions")
                    .max_height((height - 120.0).max(80.0))
                    .show(ui, |ui| {
                        let mut visible = 0;
                        for t in self.threads.iter().filter(|t| {
                            filter.is_empty()
                                || format!("{} {}", t.title, t.model)
                                    .to_lowercase()
                                    .contains(&filter)
                        }) {
                            visible += 1;
                            let selected = self.selected.as_ref() == Some(&t.id);
                            let response = Frame::NONE
                                .fill(if selected { TEAL_BG } else { Color32::WHITE })
                                .stroke(Stroke::new(
                                    1.0,
                                    if selected {
                                        Color32::from_rgb(152, 208, 193)
                                    } else {
                                        BORDER
                                    },
                                ))
                                .corner_radius(CornerRadius::same(9))
                                .inner_margin(Margin::same(12))
                                .show(ui, |ui| {
                                    ui.set_width(list_width - 62.0);
                                    let title = if t.title.is_empty() { &t.id } else { &t.title };
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(title).strong().size(14.0).color(INK),
                                        )
                                        .wrap()
                                        .sense(Sense::click()),
                                    );
                                    text(ui, format!("{} · {}", t.model, t.effort));
                                    let probe = self
                                        .history
                                        .iter()
                                        .find(|p| p.thread_id.as_deref() == Some(&t.id));
                                    if let Some(p) = probe {
                                        let (color, fill) = tone(p, &self.account.id);
                                        badge(
                                            ui,
                                            if p.account_id != self.account.id {
                                                "账户待核实"
                                            } else {
                                                p.label()
                                            },
                                            color,
                                            fill,
                                        );
                                    } else {
                                        text(ui, "尚未主动检测");
                                    }
                                });
                            let hit = ui.interact(
                                response.response.rect,
                                ui.id().with(&t.id),
                                Sense::click(),
                            );
                            if hit.clicked() {
                                self.selected = Some(t.id.clone());
                            }
                            if hit.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            ui.add_space(8.0);
                        }
                        if visible == 0 {
                            text(
                                ui,
                                if self.threads.is_empty() {
                                    "暂无已保存会话。先在 Codex 完成一轮对话，再刷新。"
                                } else {
                                    "没有匹配的会话"
                                },
                            );
                        }
                    });
            });
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width().max(260.0));
                egui::ScrollArea::vertical()
                    .id_salt("session_details")
                    .max_height(height)
                    .show(ui, |ui| {
                        if let Some(t) = self
                            .threads
                            .iter()
                            .find(|t| Some(&t.id) == self.selected.as_ref())
                            .cloned()
                        {
                            self.session_detail(ui, &t);
                        } else {
                            card(ui, |ui| {
                                ui.heading("从左侧选择一个会话");
                                text(ui, "每次检测都会明确记录目标会话与客户端来源。");
                            });
                        }
                        ui.add_space(12.0);
                        self.fresh_ui(ui);
                    });
            });
        });
    }
    fn session_detail(&mut self, ui: &mut egui::Ui, t: &Thread) {
        card(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                RichText::new(if t.title.is_empty() { &t.id } else { &t.title })
                    .size(20.0)
                    .strong()
                    .color(INK),
            );
            ui.horizontal_wrapped(|ui| {
                badge(ui, &t.model, INK, BG);
                badge(ui, &t.effort, MUTED, BG);
                text(
                    ui,
                    if t.originator.is_empty() {
                        "客户端未记录"
                    } else {
                        &t.originator
                    },
                );
            });
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        self.task.is_none() && self.binary.is_some(),
                        button(format!("主动检测 · {} 份回答", self.cfg.queries), true),
                    )
                    .clicked()
                {
                    self.start_probe(ui.ctx(), Target::Thread(t.clone()));
                }
                if ui
                    .add_enabled(self.task.is_none(), button("扫描本地日志", false))
                    .clicked()
                {
                    self.scan(ui.ctx());
                }
            });
            text(
                ui,
                "扫描日志不消耗推理额度；主动检测通过临时副本请求模型回答。",
            );
            egui::CollapsingHeader::new("会话信息")
                .id_salt(format!("meta-{}", t.id))
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(format!("会话 ID：{}", t.id))
                            .selectable(true)
                            .wrap(),
                    );
                    ui.add(
                        egui::Label::new(format!("工作目录：{}", t.cwd))
                            .selectable(true)
                            .wrap(),
                    );
                    text(ui, format!("客户端来源：{}", t.originator));
                });
        });
        ui.add_space(12.0);
        let probes = self
            .history
            .iter()
            .filter(|p| p.thread_id.as_deref() == Some(&t.id))
            .collect::<Vec<_>>();
        card(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new("最新主动检测").strong().size(16.0));
            ui.add_space(8.0);
            if let Some(p) = probes.first() {
                probe_ui(ui, p, &self.account.id);
            } else {
                text(
                    ui,
                    "尚未主动检测。运行一次检测后，会显示指纹候选、有效样本和结果依据。",
                );
            }
            if probes.len() > 1 {
                egui::CollapsingHeader::new(format!("此前检测 · {} 次", probes.len() - 1))
                    .id_salt(format!("history-{}", t.id))
                    .show(ui, |ui| {
                        for p in probes.iter().skip(1).take(7) {
                            egui::CollapsingHeader::new(format!(
                                "{} · {}",
                                time_label(&p.finished),
                                p.label()
                            ))
                            .id_salt(&p.id)
                            .show(ui, |ui| probe_ui(ui, p, &self.account.id));
                        }
                    });
            }
        });
        ui.add_space(12.0);
        card(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("本地记录").strong().size(16.0));
                badge(ui, "零推理额度", ACCENT, TEAL_BG);
            });
            if let Some(error) = self.scan_errors.get(&t.id) {
                ui.colored_label(RED, format!("本次扫描未完成：{error}"));
            }
            if let Some(scan) = self.scans.get(&t.id) {
                ui.add_space(6.0);
                text(
                    ui,
                    format!(
                        "已读取 {} 轮 · 上下文窗口 {} · 服务层级 {}",
                        scan.turns,
                        scan.ctx.map_or("未记录".into(), |c| c.to_string()),
                        scan.tier.as_deref().unwrap_or("未记录")
                    ),
                );
                if scan.active().is_empty() {
                    ui.label(RichText::new("未发现仍有效的模型、思考强度或窗口变化").color(INK));
                    text(ui, "本地记录无法证明服务端模型权重未被替换。");
                }
                for evidence in scan.active() {
                    ui.colored_label(
                        if evidence.severity == "hard" {
                            RED
                        } else {
                            AMBER
                        },
                        format!("[{}] {}", severity(&evidence.severity), evidence.detail),
                    );
                }
                egui::CollapsingHeader::new(format!("查看全部证据 · {} 条", scan.evidence.len()))
                    .id_salt(format!("evidence-{}", t.id))
                    .show(ui, |ui| {
                        for e in scan.evidence.iter().rev() {
                            ui.label(format!(
                                "{}  [{}] {}",
                                time_label(&e.ts),
                                severity(&e.severity),
                                e.detail
                            ));
                        }
                    });
            } else {
                text(ui, "尚无扫描结果");
            }
        });
    }
    fn fresh_ui(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            ui.set_min_width(ui.available_width());
            egui::CollapsingHeader::new("检测全新会话").show(ui, |ui| {
                text(
                    ui,
                    "创建无聊天历史的临时会话，明确选择模型、思考强度和客户端来源。",
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.fresh_model)
                        .hint_text("模型 ID")
                        .desired_width(f32::INFINITY),
                );
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("fresh_effort")
                        .selected_text(&self.fresh_effort)
                        .show_ui(ui, |ui| {
                            for effort in is_gpt_nerfed::scanner::EFFORTS {
                                ui.selectable_value(
                                    &mut self.fresh_effort,
                                    effort.to_string(),
                                    *effort,
                                );
                            }
                        });
                    egui::ComboBox::from_id_salt("fresh_client")
                        .selected_text(&self.fresh_originator)
                        .show_ui(ui, |ui| {
                            for origin in ["Codex Desktop", "codex_cli_rs"] {
                                ui.selectable_value(
                                    &mut self.fresh_originator,
                                    origin.to_string(),
                                    origin,
                                );
                            }
                        });
                });
                if ui
                    .add_enabled(
                        self.task.is_none()
                            && self.binary.is_some()
                            && !self.fresh_model.trim().is_empty(),
                        button("开始新会话检测", true),
                    )
                    .clicked()
                {
                    self.start_probe(
                        ui.ctx(),
                        Target::Fresh {
                            model: self.fresh_model.trim().into(),
                            effort: self.fresh_effort.clone(),
                            originator: self.fresh_originator.clone(),
                        },
                    );
                }
            });
        });
    }
    fn history_ui(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("history_page")
            .show(ui, |ui| {
                if self.history.is_empty() {
                    card(ui, |ui| {
                        ui.heading("还没有主动检测记录");
                        text(ui, "完成一次检测后，结果会保存在这里。");
                    });
                }
                for p in &self.history {
                    card(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            let (color, fill) = tone(p, &self.account.id);
                            badge(ui, p.label(), color, fill);
                            ui.label(RichText::new(&p.expected).strong().color(INK));
                            text(
                                ui,
                                format!(
                                    "{} · {}",
                                    time_label(&p.finished),
                                    if p.mode == "fresh" {
                                        "全新会话"
                                    } else {
                                        "已有会话"
                                    }
                                ),
                            );
                        });
                        egui::CollapsingHeader::new("查看结果与证据")
                            .id_salt(&p.id)
                            .show(ui, |ui| probe_ui(ui, p, &self.account.id));
                    });
                    ui.add_space(12.0);
                }
            });
    }
    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("settings_page").show(ui, |ui| {
            card(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(RichText::new("Codex 连接").strong().size(17.0)); ui.add_space(8.0);
                if let Some(binary) = &self.binary { badge(ui, &format!("Codex {}", binary.version), ACCENT, TEAL_BG); text(ui, binary.path.display().to_string()); }
                else { badge(ui, "尚未连接", AMBER, AMBER_BG); text(ui, "安装并登录 Codex 后，点击刷新状态。"); }
                text(ui, &self.account.label);
                ui.add_space(12.0);
                ui.label(RichText::new("程序路径").strong());
                text(ui, "留空时自动选择发现的最新版本。支持 EXE、CMD、BAT 和 PS1。");
                ui.add(egui::TextEdit::singleline(&mut self.draft.codex_bin).desired_width(f32::INFINITY).hint_text("自动发现 Codex"));
            });
            ui.add_space(12.0);
            card(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(RichText::new("检测计划").strong().size(17.0)); ui.add_space(8.0);
                let was = self.draft.automatic;
                if ui.checkbox(&mut self.draft.automatic, "开启后台主动检测").changed() && !was && self.draft.automatic { self.draft.automatic = false; self.confirm_auto = true; }
                text(ui, "只检测最近 15 分钟有活动、且上次检测后有新活动的用户会话。关闭计划不打断正在运行的检测。");
                ui.add_space(6.0);
                egui::Grid::new("schedule").spacing([28.0, 12.0]).show(ui, |ui| {
                    ui.label("检测间隔"); ui.horizontal(|ui| { ui.add(egui::DragValue::new(&mut self.draft.interval_minutes).range(5..=1440)); text(ui, "分钟"); }); ui.end_row();
                    ui.label("回答数量"); ui.horizontal(|ui| { ui.add(egui::DragValue::new(&mut self.draft.queries).range(1..=3)); text(ui, "每次 1–3 份"); }); ui.end_row();
                    ui.label("单轮超时"); ui.horizontal(|ui| { ui.add(egui::DragValue::new(&mut self.draft.timeout_seconds).range(30..=600)); text(ui, "秒"); }); ui.end_row();
                });
                ui.add_space(10.0);
                text(ui, "主动探测消耗你的 Codex 额度；超时样本最多补充一次。单份回答不足以作高置信度的不匹配判定。");
                ui.checkbox(&mut self.draft.notifications, "可疑、不匹配或模型未收录时显示 Windows 通知");
                ui.add_space(8.0);
                if ui.add_enabled(self.task.is_none(), button("保存设置", true)).clicked() { self.save(ui.ctx()); }
            });
            ui.add_space(12.0);
            card(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(RichText::new("后台调度与 hooks").strong().size(17.0));
                ui.add_space(8.0); badge(ui, "独立调度 · 无需 hooks", ACCENT, TEAL_BG);
                text(ui, "本程序每 30 秒刷新本地会话；托盘状态下调度继续运行。退出程序后调度停止。");
                text(ui, "私有指纹探测显式禁用 hooks，避免递归检测。不会自动修改 Codex 插件或 hook 信任配置。");
                ui.add_space(10.0);
                if let Some(status) = &self.hooks {
                    if status.hooks.is_empty() { text(ui, "Codex 中未发现 is-gpt-nerfed hooks。当前 Windows 客户端仍可正常扫描和检测。"); }
                    else {
                        for hook in &status.hooks { text(ui, format!("{} · {} · {}{}", hook.event, if hook.enabled { "已启用" } else { "已禁用" }, if hook.trusted { "已信任" } else { "未信任" }, if hook.legacy { " · 旧 Python/POSIX 入口" } else { "" })); }
                        if status.hooks.iter().any(|h| h.legacy) { ui.colored_label(AMBER, "发现旧版入口。它不属于此 Windows 客户端，注册或信任成功也不能证明它已在 Windows 中运行。"); }
                    }
                    for note in &status.notes { ui.colored_label(AMBER, note); }
                    text(ui, format!("最近检查：{}。点击“刷新状态”重新检查。", time_label(&status.checked_at)));
                } else if let Some(error) = &self.hook_error { ui.colored_label(AMBER, format!("hook 状态检查未完成：{error}")); }
                else { text(ui, "连接 Codex 后可读取实际 hook 注册与信任状态。"); }
            });
            ui.add_space(12.0);
            card(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(RichText::new("本地数据").strong().size(17.0)); text(ui, self.store.home.display().to_string());
                if ui.add(button("打开数据目录", false)).clicked() && let Err(e) = platform::open_folder(&self.store.home) { self.error = Some(e.to_string()); }
                text(ui, "关闭窗口会收起到托盘；退出后删除 EXE 即可卸载，历史仍保留在上述目录。");
                egui::CollapsingHeader::new("许可证与方法来源").show(ui, |ui| {
                    text(ui, "ModelTrace 指纹库与评分方法：xqy2006（MIT）。指纹归因只能作为辅助证据。");
                    text(ui, include_str!("../../plugin/assets/modeltrace/LICENSE-ModelTrace.txt"));
                    text(ui, include_str!("../../LICENSE"));
                });
            });
        });
    }
}
fn severity(value: &str) -> &str {
    match value {
        "hard" => "日志硬证据",
        "soft" => "需确认",
        "good" => "升级记录",
        _ => "信息",
    }
}
fn probe_ui(ui: &mut egui::Ui, p: &Probe, account: &str) {
    let (color, fill) = tone(p, account);
    ui.horizontal_wrapped(|ui| {
        badge(ui, p.label(), color, fill);
        text(ui, &p.assessment.verdict);
    });
    ui.add_space(8.0);
    if p.account_id != account || p.account_id.is_empty() {
        ui.colored_label(AMBER, "此结果来自其他账户或账户未知，不能代表当前账户。");
    }
    if p.status == "cancelled" {
        text(ui, "检测已取消，不能得出结论。");
    }
    match p.assessment.verdict.as_str() {
        "UNLISTED" => {
            ui.label(
                RichText::new("该模型未被指纹库收录")
                    .size(20.0)
                    .strong()
                    .color(INK),
            );
            text(ui, "无法据此判断是否降级，最近似候选也不等于实际模型身份。");
        }
        "MATCH" => {
            ui.label(
                RichText::new("指纹与所选模型一致")
                    .size(20.0)
                    .strong()
                    .color(INK),
            );
            text(ui, "这是一项指纹归因结果，不是服务端模型身份的直接证明。");
        }
        "INVALID" => {
            ui.label(
                RichText::new("本次未取得有效结论")
                    .size(20.0)
                    .strong()
                    .color(INK),
            );
        }
        _ => {
            ui.label(RichText::new(p.label()).size(20.0).strong().color(INK));
        }
    }
    ui.add_space(8.0);
    if let Some(a) = &p.analysis {
        ui.label(
            RichText::new(format!("最近似候选   {}", a.prediction))
                .strong()
                .color(INK),
        );
        ui.horizontal(|ui| {
            text(ui, "归因概率");
            ui.label(
                RichText::new(format!("{:.1}%", a.probability * 100.0))
                    .strong()
                    .color(color),
            );
        });
        text(ui, "候选仅供参考，尤其在所选模型未收录时。");
    }
    text(
        ui,
        format!(
            "有效回答 {}/{}    耗时 {:.1} 秒    {}",
            p.used_outputs,
            p.queries,
            p.elapsed_s,
            time_label(&p.finished)
        ),
    );
    for error in &p.errors {
        ui.colored_label(RED, error);
    }
    egui::CollapsingHeader::new("检测依据与明细")
        .id_salt(format!("probe-{}", p.id))
        .show(ui, |ui| {
            text(ui, format!("所选模型：{}", p.expected));
            text(ui, format!("客户端：{} · Codex {}", p.originator, p.codex));
            ui.add(
                egui::Label::new(format!("检测编号：{}", p.id))
                    .selectable(true)
                    .wrap(),
            );
            if let Some(id) = &p.thread_id {
                ui.add(
                    egui::Label::new(format!("目标会话：{id}"))
                        .selectable(true)
                        .wrap(),
                );
            }
            if let Some(a) = &p.analysis {
                for candidate in a.results.iter().take(6) {
                    text(
                        ui,
                        format!(
                            "{} · {:.2}% · 评分 {:.4}",
                            candidate.model,
                            candidate.probability * 100.0,
                            candidate.score
                        ),
                    );
                }
            }
            for e in &p.hard_evidence {
                ui.colored_label(RED, format!("日志硬证据：{}", e.detail));
            }
        });
}
pub(super) fn setup(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let root = std::env::var("WINDIR").unwrap_or_else(|_| "C:/Windows".into());
    for name in ["msyh.ttc", "msyh.ttf", "simhei.ttf"] {
        if let Ok(bytes) = std::fs::read(format!("{root}/Fonts/{name}")) {
            fonts.font_data.insert(
                "chinese".into(),
                Arc::new(egui::FontData::from_owned(bytes)),
            );
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "chinese".into());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push("chinese".into());
            break;
        }
    }
    ctx.set_fonts(fonts);
    ctx.set_visuals(egui::Visuals::light());
    let mut style = (*ctx.style_of(egui::Theme::Light)).clone();
    style.spacing.item_spacing = Vec2::new(12.0, 9.0);
    style.spacing.button_padding = Vec2::new(12.0, 8.0);
    style.visuals.override_text_color = Some(INK);
    style.visuals.panel_fill = BG;
    style.visuals.selection.bg_fill = TEAL_BG;
    style.visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    style.visuals.widgets.inactive.bg_fill = Color32::WHITE;
    style.visuals.widgets.inactive.weak_bg_fill = Color32::WHITE;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    style.visuals.widgets.hovered.bg_fill = TEAL_BG;
    style.visuals.widgets.hovered.weak_bg_fill = TEAL_BG;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(149, 197, 183));
    style.visuals.widgets.active.bg_fill = TEAL_BG;
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    for widget in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
    ] {
        widget.corner_radius = CornerRadius::same(7);
    }
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, FontId::proportional(20.0));
    ctx.set_style_of(egui::Theme::Light, style);
}

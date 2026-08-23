fn instruction_source<'a>(
    dto: &'a crate::domain::GlobalInstructionDto,
    source: &str,
) -> &'a crate::domain::GlobalInstructionSourceRow {
    dto.sources
        .iter()
        .find(|row| row.source == source)
        .unwrap_or_else(|| panic!("missing source {source}"))
}

fn file_named<'a>(
    row: &'a crate::domain::GlobalInstructionSourceRow,
    display_path: &str,
) -> &'a crate::domain::GlobalInstructionFile {
    row.files
        .iter()
        .find(|file| file.display_path == display_path)
        .unwrap_or_else(|| panic!("missing file {display_path}"))
}

#[test]
fn scan_lists_claude_main_file_and_user_instruction_files() {
    let home = tempfile::tempdir().unwrap();
    let claude = home.path().join(".claude");
    let user_dir = claude.join("rules");
    std::fs::create_dir_all(&user_dir).unwrap();
    std::fs::write(claude.join("CLAUDE.md"), "prefer-chinese\n").unwrap();
    std::fs::write(user_dir.join("routing.md"), "# routing\n").unwrap();
    std::fs::write(user_dir.join("skills.md"), "# skills\n").unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        Some(home.path().join("proj").as_path()),
        &crate::domain::InstructionUsageSummary::default(),
    );

    let claude = instruction_source(&dto, "claude");
    assert_eq!(claude.application, "Claude");
    let main = file_named(claude, "~/.claude/CLAUDE.md");
    assert_eq!(main.byte_size, 15);
    assert_eq!(main.content, "prefer-chinese\n");
    assert_eq!(
        main.load_status,
        crate::domain::InstructionLoadStatus::Loaded
    );
    assert!(main.modified_at.as_deref().is_some_and(|t| !t.is_empty()));
    let routing = file_named(claude, "~/.claude/rules/routing.md");
    assert_eq!(routing.content, "# routing\n");
    assert_eq!(
        routing.load_status,
        crate::domain::InstructionLoadStatus::Loaded
    );
    assert_eq!(
        file_named(claude, "~/.claude/rules/skills.md").content,
        "# skills\n"
    );
}

#[test]
fn scan_lists_claude_rules_directory_when_present() {
    let home = tempfile::tempdir().unwrap();
    let claude_rules = home.path().join(".claude").join("rules");
    let codex_rules = home.path().join(".codex").join("rules");
    std::fs::create_dir_all(&claude_rules).unwrap();
    std::fs::create_dir_all(&codex_rules).unwrap();
    std::fs::write(home.path().join(".claude/CLAUDE.md"), "ok\n").unwrap();
    std::fs::write(claude_rules.join("routing.md"), "# routing\n").unwrap();
    std::fs::write(codex_rules.join("default.rules"), "third-party\n").unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );

    let claude_dir = file_named(instruction_source(&dto, "claude"), "~/.claude/rules/");
    assert_eq!(
        claude_dir.kind,
        crate::domain::InstructionEntryKind::Directory
    );
    assert_eq!(
        claude_dir.load_status,
        crate::domain::InstructionLoadStatus::Loaded
    );
    assert_eq!(claude_dir.abs_path, claude_rules.to_string_lossy());
    assert!(instruction_source(&dto, "codex")
        .files
        .iter()
        .all(|file| file.kind != crate::domain::InstructionEntryKind::Directory));
}

#[test]
fn scan_marks_missing_claude_main_file_not_created() {
    let home = tempfile::tempdir().unwrap();
    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    let files = &instruction_source(&dto, "claude").files;
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].display_path, "~/.claude/CLAUDE.md");
    assert_eq!(
        files[0].load_status,
        crate::domain::InstructionLoadStatus::NotCreated
    );
    assert_eq!(files[0].byte_size, 0);
    assert_eq!(files[0].content, "");
    assert!(files[0].modified_at.is_none());
}

#[test]
fn scan_reports_usage_share_high_and_loaded_bytes_low() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    std::fs::write(home.path().join(".claude/CLAUDE.md"), "thin\n").unwrap();
    std::fs::write(home.path().join(".codex/AGENTS.md"), vec![b'a'; 8_000]).unwrap();
    let usage = crate::domain::InstructionUsageSummary {
        sources: vec![
            crate::domain::InstructionSourceUsage {
                source: "claude".into(),
                total_tokens: 80_000,
            },
            crate::domain::InstructionSourceUsage {
                source: "codex".into(),
                total_tokens: 20_000,
            },
        ],
    };

    let dto = crate::instructions::scan(home.path(), None, &usage);
    let claude = dto
        .investments
        .iter()
        .find(|row| row.source == "claude")
        .expect("claude investment");
    assert_eq!(claude.loaded_bytes, 5);
    assert_eq!(claude.total_tokens, 80_000);
    assert_eq!(dto.imbalances.len(), 1);
    assert_eq!(dto.imbalances[0].source, "claude");
    assert!(dto.imbalances[0].note.contains("80"));
    assert!(dto.imbalances[0].note.contains("5"));
    assert!(!dto.imbalances[0].note.contains("未修改"));
}

#[test]
fn scan_does_not_report_imbalance_when_high_usage_has_substantial_instructions() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    std::fs::write(home.path().join(".claude/CLAUDE.md"), vec![b'a'; 8_000]).unwrap();
    std::fs::write(home.path().join(".codex/AGENTS.md"), "thin\n").unwrap();
    let usage = crate::domain::InstructionUsageSummary {
        sources: vec![
            crate::domain::InstructionSourceUsage {
                source: "claude".into(),
                total_tokens: 80_000,
            },
            crate::domain::InstructionSourceUsage {
                source: "codex".into(),
                total_tokens: 20_000,
            },
        ],
    };

    let dto = crate::instructions::scan(home.path(), None, &usage);
    assert!(dto.imbalances.is_empty());
}

#[test]
fn scan_stays_quiet_when_usage_is_zero() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::fs::write(home.path().join(".claude/CLAUDE.md"), "thin\n").unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    assert!(dto.imbalances.is_empty());
    assert!(dto
        .investments
        .iter()
        .any(|row| row.source == "claude" && row.loaded_bytes == 5));
}

#[test]
fn scan_reports_imbalance_when_sole_high_usage_source_is_thin() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::fs::write(home.path().join(".claude/CLAUDE.md"), "thin\n").unwrap();
    let usage = crate::domain::InstructionUsageSummary {
        sources: vec![crate::domain::InstructionSourceUsage {
            source: "claude".into(),
            total_tokens: 99_000,
        }],
    };

    let dto = crate::instructions::scan(home.path(), None, &usage);
    assert_eq!(dto.imbalances.len(), 1);
    assert_eq!(dto.imbalances[0].source, "claude");
}

#[test]
fn scan_stays_quiet_when_total_usage_is_too_small() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    std::fs::write(home.path().join(".claude/CLAUDE.md"), "thin\n").unwrap();
    std::fs::write(home.path().join(".codex/AGENTS.md"), "thin\n").unwrap();
    let usage = crate::domain::InstructionUsageSummary {
        sources: vec![
            crate::domain::InstructionSourceUsage {
                source: "claude".into(),
                total_tokens: 80,
            },
            crate::domain::InstructionSourceUsage {
                source: "codex".into(),
                total_tokens: 20,
            },
        ],
    };

    let dto = crate::instructions::scan(home.path(), None, &usage);
    assert!(dto.imbalances.is_empty());
}

#[test]
fn scan_does_not_flag_sources_without_a_global_instruction_mechanism() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::fs::write(home.path().join(".claude/CLAUDE.md"), vec![b'a'; 8_000]).unwrap();
    let usage = crate::domain::InstructionUsageSummary {
        sources: vec![
            crate::domain::InstructionSourceUsage {
                source: "kimi".into(),
                total_tokens: 80_000,
            },
            crate::domain::InstructionSourceUsage {
                source: "claude".into(),
                total_tokens: 20_000,
            },
        ],
    };

    let dto = crate::instructions::scan(home.path(), None, &usage);
    assert!(dto.imbalances.iter().all(|item| item.source != "kimi"));
}

#[test]
fn scan_reports_keyword_overlap_between_global_and_project_rules() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::fs::write(
        home.path().join(".claude/CLAUDE.md"),
        "样式统一：优先使用 Tailwind CSS，避免自定义 CSS\n",
    )
    .unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::write(
        project.path().join("AGENTS.md"),
        "没有引入 Tailwind CSS，不要引入新的样式方案\n",
    )
    .unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        Some(project.path()),
        &crate::domain::InstructionUsageSummary::default(),
    );

    assert_eq!(
        dto.selected_project.as_deref(),
        Some(project.path().to_str().unwrap())
    );
    assert_eq!(dto.hints.len(), 1);
    let hint = &dto.hints[0];
    assert_eq!(hint.keyword, "tailwind");
    assert_eq!(hint.global_application, "Claude");
    assert_eq!(hint.global_display_path, "~/.claude/CLAUDE.md");
    assert!(hint.global_snippet.contains("Tailwind"));
    assert_eq!(hint.project_display_path, "AGENTS.md");
    assert!(hint.project_snippet.contains("Tailwind"));
}

#[test]
fn scan_reports_chinese_keyword_overlap() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::fs::write(
        home.path().join(".claude/CLAUDE.md"),
        "原则：向后兼容，不得破坏现有接口。\n",
    )
    .unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("AGENTS.md"), "铁律：向后兼容。\n").unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        Some(project.path()),
        &crate::domain::InstructionUsageSummary::default(),
    );

    assert!(
        dto.hints.iter().any(|hint| hint.keyword == "向后兼容"),
        "{:?}",
        dto.hints
    );
}

#[test]
fn scan_does_not_report_overlap_when_keywords_differ() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::fs::write(home.path().join(".claude/CLAUDE.md"), "prefer-chinese\n").unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("CLAUDE.md"), "prefer-tabs\n").unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        Some(project.path()),
        &crate::domain::InstructionUsageSummary::default(),
    );
    assert!(dto.hints.is_empty());
}

#[test]
fn scan_reports_no_hints_without_project_root() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::fs::write(
        home.path().join(".claude/CLAUDE.md"),
        "优先使用 Tailwind CSS\n",
    )
    .unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    assert!(dto.selected_project.is_none());
    assert!(dto.hints.is_empty());
}

#[test]
fn scan_reads_cursor_rules_under_selected_project() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::fs::write(
        home.path().join(".claude/CLAUDE.md"),
        "优先使用 Tailwind CSS\n",
    )
    .unwrap();
    let project = tempfile::tempdir().unwrap();
    let rules = project.path().join(".cursor/rules");
    std::fs::create_dir_all(&rules).unwrap();
    std::fs::write(rules.join("ui.mdc"), "没有引入 Tailwind CSS\n").unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        Some(project.path()),
        &crate::domain::InstructionUsageSummary::default(),
    );
    assert_eq!(dto.hints[0].project_display_path, ".cursor/rules/ui.mdc");
}

#[test]
fn scan_for_projects_defaults_to_first_existing_directory() {
    let home = tempfile::tempdir().unwrap();
    let existing = tempfile::tempdir().unwrap();
    let missing = existing.path().join("gone");
    let recent = [
        missing.to_string_lossy().into_owned(),
        existing.path().to_string_lossy().into_owned(),
    ];

    let dto = crate::instructions::scan_for_projects(
        home.path(),
        None,
        &recent,
        &crate::domain::InstructionUsageSummary::default(),
    );

    assert_eq!(dto.selected_project.as_deref(), existing.path().to_str());
    assert_eq!(
        dto.projects,
        vec![existing.path().to_string_lossy().into_owned()]
    );
}

#[test]
fn scan_codex_override_shields_base_agents_file() {
    let home = tempfile::tempdir().unwrap();
    let codex = home.path().join(".codex");
    std::fs::create_dir_all(&codex).unwrap();
    std::fs::write(codex.join("AGENTS.md"), "base-instruction\n").unwrap();
    std::fs::write(codex.join("AGENTS.override.md"), "override-instruction\n").unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    let row = instruction_source(&dto, "codex");
    let base = file_named(row, "~/.codex/AGENTS.md");
    let over = file_named(row, "~/.codex/AGENTS.override.md");
    assert_eq!(
        over.load_status,
        crate::domain::InstructionLoadStatus::Loaded
    );
    assert_eq!(over.content, "override-instruction\n");
    assert_eq!(
        base.load_status,
        crate::domain::InstructionLoadStatus::PresentUnloaded
    );
    assert_eq!(base.content, "base-instruction\n");
    assert!(base
        .note
        .as_deref()
        .is_some_and(|note| note.contains("AGENTS.override.md")));
}

#[test]
fn scan_codex_rules_dir_is_present_unloaded() {
    let home = tempfile::tempdir().unwrap();
    let rules = home.path().join(".codex/rules");
    std::fs::create_dir_all(&rules).unwrap();
    std::fs::write(rules.join("default.rules"), "third-party\n").unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    let extra = file_named(
        instruction_source(&dto, "codex"),
        "~/.codex/rules/default.rules",
    );
    assert_eq!(
        extra.load_status,
        crate::domain::InstructionLoadStatus::PresentUnloaded
    );
    assert_eq!(extra.content, "third-party\n");
    assert!(extra
        .note
        .as_deref()
        .is_some_and(|note| note.contains("第三方")));
}

#[test]
fn scan_gemini_missing_file_is_not_created_not_absent() {
    let home = tempfile::tempdir().unwrap();
    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    let files = &instruction_source(&dto, "gemini").files;
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].display_path, "~/.gemini/GEMINI.md");
    assert_eq!(
        files[0].load_status,
        crate::domain::InstructionLoadStatus::NotCreated
    );
}

#[test]
fn scan_cursor_account_preference_is_locally_invisible() {
    let home = tempfile::tempdir().unwrap();
    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    let files = &instruction_source(&dto, "cursor").files;
    assert_eq!(files.len(), 1);
    assert_eq!(
        files[0].load_status,
        crate::domain::InstructionLoadStatus::LocallyInvisible
    );
    assert!(files[0]
        .note
        .as_deref()
        .is_some_and(|note| note.contains("账号服务端")));
    assert_eq!(files[0].action.as_deref(), Some("cursor_settings"));
}

#[test]
fn scan_covers_every_supported_source() {
    let home = tempfile::tempdir().unwrap();
    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    let names: Vec<&str> = dto.sources.iter().map(|row| row.source.as_str()).collect();
    assert_eq!(
        names,
        [
            "claude",
            "codex",
            "gemini",
            "cursor",
            "pi",
            "opencode",
            "kimi",
            "dsh",
            "grok",
            "qwen",
            "factory",
            "cursor_agent",
            "copilot",
        ]
    );
    for row in &dto.sources {
        assert!(!row.files.is_empty(), "{} should not be absent", row.source);
    }
}

#[test]
fn scan_remaining_sources_use_documented_evidence() {
    let home = tempfile::tempdir().unwrap();
    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );

    let pi = file_named(instruction_source(&dto, "pi"), "~/.pi/agent/AGENTS.md");
    assert_eq!(pi.evidence, crate::domain::InstructionEvidence::Verified);
    assert_eq!(
        pi.load_status,
        crate::domain::InstructionLoadStatus::NotCreated
    );

    let opencode = file_named(
        instruction_source(&dto, "opencode"),
        "~/.config/opencode/AGENTS.md",
    );
    assert_eq!(
        opencode.evidence,
        crate::domain::InstructionEvidence::Verified
    );
    assert_eq!(
        opencode.load_status,
        crate::domain::InstructionLoadStatus::NotCreated
    );

    let kimi = &instruction_source(&dto, "kimi").files[0];
    assert_eq!(
        kimi.evidence,
        crate::domain::InstructionEvidence::NoMechanism
    );
    assert!(kimi.abs_path.is_empty());
    assert!(
        !kimi.display_path.contains('/'),
        "无机制条目不得给出可创建的假路径"
    );

    let dsh = file_named(instruction_source(&dto, "dsh"), "~/.dsh/AGENTS.md");
    assert_eq!(dsh.evidence, crate::domain::InstructionEvidence::Verified);

    let grok = file_named(instruction_source(&dto, "grok"), "~/.grok/AGENTS.md");
    assert_eq!(grok.evidence, crate::domain::InstructionEvidence::Verified);

    let qwen = file_named(instruction_source(&dto, "qwen"), "~/.qwen/QWEN.md");
    assert_eq!(qwen.evidence, crate::domain::InstructionEvidence::Verified);

    let factory = file_named(instruction_source(&dto, "factory"), "~/.factory/AGENTS.md");
    assert_eq!(
        factory.evidence,
        crate::domain::InstructionEvidence::Verified
    );

    let cursor_agent = &instruction_source(&dto, "cursor_agent").files[0];
    assert_eq!(
        cursor_agent.evidence,
        crate::domain::InstructionEvidence::Inferred
    );
    assert_eq!(
        cursor_agent.load_status,
        crate::domain::InstructionLoadStatus::LocallyInvisible
    );
    assert!(cursor_agent.action.is_none());
    assert!(
        cursor_agent
            .note
            .as_deref()
            .is_some_and(|note| note.contains("推测")),
        "推测条目必须说明尚未证实"
    );

    let copilot = file_named(
        instruction_source(&dto, "copilot"),
        "~/.copilot/copilot-instructions.md",
    );
    assert_eq!(
        copilot.evidence,
        crate::domain::InstructionEvidence::Verified
    );
}

#[test]
fn scan_reads_verified_remaining_instruction_files() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".pi/agent")).unwrap();
    std::fs::write(home.path().join(".pi/agent/AGENTS.md"), "pi-global\n").unwrap();
    std::fs::create_dir_all(home.path().join(".config/opencode")).unwrap();
    std::fs::write(
        home.path().join(".config/opencode/AGENTS.md"),
        "opencode-global\n",
    )
    .unwrap();
    std::fs::create_dir_all(home.path().join(".qwen")).unwrap();
    std::fs::write(home.path().join(".qwen/QWEN.md"), "qwen-global\n").unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    assert_eq!(
        file_named(instruction_source(&dto, "pi"), "~/.pi/agent/AGENTS.md").content,
        "pi-global\n"
    );
    assert_eq!(
        file_named(
            instruction_source(&dto, "opencode"),
            "~/.config/opencode/AGENTS.md"
        )
        .load_status,
        crate::domain::InstructionLoadStatus::Loaded
    );
    assert_eq!(
        file_named(instruction_source(&dto, "qwen"), "~/.qwen/QWEN.md").content,
        "qwen-global\n"
    );
}

#[test]
fn scan_pi_override_shields_base_agents_file() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".pi/agent");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("AGENTS.md"), "base-pi\n").unwrap();
    std::fs::write(dir.join("AGENTS.override.md"), "override-pi\n").unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    let row = instruction_source(&dto, "pi");
    let base = file_named(row, "~/.pi/agent/AGENTS.md");
    let over = file_named(row, "~/.pi/agent/AGENTS.override.md");
    assert_eq!(
        over.load_status,
        crate::domain::InstructionLoadStatus::Loaded
    );
    assert_eq!(over.content, "override-pi\n");
    assert_eq!(
        base.load_status,
        crate::domain::InstructionLoadStatus::PresentUnloaded
    );
    assert_eq!(base.content, "base-pi\n");
}

fn checkup_named<'a>(
    dto: &'a crate::domain::GlobalInstructionDto,
    kind: crate::domain::InstructionCheckupKind,
    display_path: &str,
) -> &'a crate::domain::InstructionCheckupFinding {
    dto.findings
        .iter()
        .find(|finding| finding.kind == kind && finding.display_path == display_path)
        .unwrap_or_else(|| panic!("missing finding {kind:?} {display_path}"))
}

#[test]
fn scan_reports_empty_loaded_file() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".gemini")).unwrap();
    std::fs::write(home.path().join(".gemini/GEMINI.md"), "").unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    let finding = checkup_named(
        &dto,
        crate::domain::InstructionCheckupKind::Empty,
        "~/.gemini/GEMINI.md",
    );
    assert_eq!(
        finding.severity,
        crate::domain::InstructionCheckupSeverity::High
    );
    assert!(finding.problem.contains("0") || finding.problem.contains("空"));
    assert!(!finding.consequence.is_empty());
}

#[test]
fn scan_does_not_report_empty_when_file_has_bytes() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".gemini")).unwrap();
    std::fs::write(home.path().join(".gemini/GEMINI.md"), "prefer-tabs\n").unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    assert!(dto
        .findings
        .iter()
        .all(|finding| { finding.kind != crate::domain::InstructionCheckupKind::Empty }));
}

#[test]
fn scan_does_not_report_empty_for_unloaded_zero_byte_file() {
    let home = tempfile::tempdir().unwrap();
    let rules = home.path().join(".codex/rules");
    std::fs::create_dir_all(&rules).unwrap();
    std::fs::write(rules.join("default.rules"), "").unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    assert!(dto
        .findings
        .iter()
        .all(|finding| { finding.kind != crate::domain::InstructionCheckupKind::Empty }));
    checkup_named(
        &dto,
        crate::domain::InstructionCheckupKind::PresentUnloaded,
        "~/.codex/rules/default.rules",
    );
}

#[test]
fn scan_reports_present_unloaded_leftover() {
    let home = tempfile::tempdir().unwrap();
    let rules = home.path().join(".codex/rules");
    std::fs::create_dir_all(&rules).unwrap();
    std::fs::write(rules.join("default.rules"), "third-party\n").unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    let finding = checkup_named(
        &dto,
        crate::domain::InstructionCheckupKind::PresentUnloaded,
        "~/.codex/rules/default.rules",
    );
    assert_eq!(
        finding.severity,
        crate::domain::InstructionCheckupSeverity::High
    );
    assert!(finding.problem.contains("不会加载"));
    assert!(finding.consequence.contains("不会改变"));
}

#[test]
fn scan_does_not_report_present_unloaded_for_loaded_file() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    std::fs::write(home.path().join(".codex/AGENTS.md"), "base\n").unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    assert!(dto
        .findings
        .iter()
        .all(|finding| { finding.kind != crate::domain::InstructionCheckupKind::PresentUnloaded }));
}

#[test]
fn scan_reports_override_shielding_base_file() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".codex");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("AGENTS.md"), "base\n").unwrap();
    std::fs::write(dir.join("AGENTS.override.md"), "override\n").unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    let finding = checkup_named(
        &dto,
        crate::domain::InstructionCheckupKind::OverrideShields,
        "~/.codex/AGENTS.md",
    );
    assert_eq!(
        finding.severity,
        crate::domain::InstructionCheckupSeverity::Medium
    );
    assert!(finding.problem.contains("屏蔽"));
    assert!(finding.consequence.contains("不会生效") || finding.consequence.contains("覆盖"));
    assert!(dto.findings.iter().all(|item| {
        item.kind != crate::domain::InstructionCheckupKind::PresentUnloaded
            || item.display_path != "~/.codex/AGENTS.md"
    }));
}

#[test]
fn scan_does_not_report_override_when_only_base_exists() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    std::fs::write(home.path().join(".codex/AGENTS.md"), "base\n").unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    assert!(dto
        .findings
        .iter()
        .all(|finding| { finding.kind != crate::domain::InstructionCheckupKind::OverrideShields }));
}

#[test]
fn scan_reports_over_limit_when_loaded_bytes_exceed_cap() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    std::fs::write(
        home.path().join(".codex/AGENTS.md"),
        vec![b'a'; 32 * 1024 + 1],
    )
    .unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    let finding = dto
        .findings
        .iter()
        .find(|item| item.kind == crate::domain::InstructionCheckupKind::OverLimit)
        .expect("over_limit");
    assert_eq!(
        finding.severity,
        crate::domain::InstructionCheckupSeverity::Critical
    );
    assert!(finding.problem.contains("超过"));
    assert!(finding.consequence.contains("截断"));
}

#[test]
fn scan_reports_near_limit_when_loaded_bytes_approach_cap() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    std::fs::write(home.path().join(".codex/AGENTS.md"), vec![b'a'; 26 * 1024]).unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    let finding = dto
        .findings
        .iter()
        .find(|item| item.kind == crate::domain::InstructionCheckupKind::NearLimit)
        .expect("near_limit");
    assert_eq!(
        finding.severity,
        crate::domain::InstructionCheckupSeverity::Low
    );
    assert!(finding.problem.contains("接近"));
    assert!(finding.consequence.contains("截断"));
}

#[test]
fn scan_reports_near_limit_when_loaded_bytes_equal_cap() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    std::fs::write(home.path().join(".codex/AGENTS.md"), vec![b'a'; 32 * 1024]).unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    assert!(dto
        .findings
        .iter()
        .all(|finding| { finding.kind != crate::domain::InstructionCheckupKind::OverLimit }));
    assert!(dto
        .findings
        .iter()
        .any(|finding| { finding.kind == crate::domain::InstructionCheckupKind::NearLimit }));
}

#[test]
fn scan_does_not_report_limit_when_loaded_bytes_are_small() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    std::fs::write(home.path().join(".codex/AGENTS.md"), "short\n").unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    assert!(dto.findings.iter().all(|finding| {
        finding.kind != crate::domain::InstructionCheckupKind::NearLimit
            && finding.kind != crate::domain::InstructionCheckupKind::OverLimit
    }));
}

#[test]
fn scan_sorts_checkup_findings_by_severity() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".codex/rules")).unwrap();
    std::fs::create_dir_all(home.path().join(".gemini")).unwrap();
    std::fs::write(
        home.path().join(".codex/AGENTS.md"),
        vec![b'a'; 32 * 1024 + 8],
    )
    .unwrap();
    std::fs::write(home.path().join(".codex/rules/default.rules"), "left\n").unwrap();
    std::fs::write(home.path().join(".gemini/GEMINI.md"), "").unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    let kinds: Vec<_> = dto.findings.iter().map(|finding| finding.kind).collect();
    assert_eq!(
        kinds,
        [
            crate::domain::InstructionCheckupKind::OverLimit,
            crate::domain::InstructionCheckupKind::Empty,
            crate::domain::InstructionCheckupKind::PresentUnloaded,
        ]
    );
}

#[test]
fn scan_emits_no_findings_when_loaded_files_are_healthy() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::fs::write(home.path().join(".claude/CLAUDE.md"), "prefer-chinese\n").unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    assert!(dto.findings.is_empty());
    assert!(dto.claude_memories.is_empty());
}

fn write_claude_auto_memory(home: &std::path::Path, slug: &str, files: &[(&str, &str)]) {
    let dir = home.join(".claude/projects").join(slug).join("memory");
    std::fs::create_dir_all(&dir).unwrap();
    for (name, content) in files {
        std::fs::write(dir.join(name), content).unwrap();
    }
}

#[test]
fn scan_lists_claude_auto_memory_for_readonly_browse() {
    let home = tempfile::tempdir().unwrap();
    write_claude_auto_memory(
        home.path(),
        "-Users-demo-app",
        &[
            (
                "MEMORY.md",
                "# Memory Index\n- [note](note.md) — fixture index\n",
            ),
            ("note.md", "fixture-note\n"),
        ],
    );
    std::fs::create_dir_all(home.path().join(".claude/projects/-Users-empty/memory")).unwrap();

    let dto = scan_home(home.path());

    assert_eq!(dto.claude_memories.len(), 1);
    let repo = &dto.claude_memories[0];
    assert_eq!(repo.repo, "/Users/demo/app");
    assert_eq!(
        repo.display_path,
        "~/.claude/projects/-Users-demo-app/memory/"
    );
    let memory_dir = home.path().join(".claude/projects/-Users-demo-app/memory");
    let expected_size = std::fs::metadata(memory_dir.join("MEMORY.md"))
        .unwrap()
        .len()
        + std::fs::metadata(memory_dir.join("note.md")).unwrap().len();
    assert_eq!(repo.byte_size, expected_size);
    assert!(repo.modified_at.as_deref().is_some_and(|t| !t.is_empty()));
    assert_eq!(
        repo.files
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>(),
        ["MEMORY.md", "note.md"]
    );
    assert_eq!(
        repo.files[0].content,
        "# Memory Index\n- [note](note.md) — fixture index\n"
    );
    assert_eq!(repo.files[1].content, "fixture-note\n");

    let finding = dto
        .findings
        .iter()
        .find(|item| item.kind == crate::domain::InstructionCheckupKind::AutoMemory)
        .expect("missing auto memory finding");
    assert_eq!(finding.source, "claude");
    assert!(finding.problem.contains("自动记忆"));
    assert!(finding.consequence.contains("注入"));
    assert!(!finding.problem.contains("fixture-note"));
    assert!(!finding.consequence.contains("fixture-note"));

    let claude = instruction_source(&dto, "claude");
    assert!(claude.files.iter().all(|file| {
        !file.display_path.contains("memory") && !file.content.contains("fixture-note")
    }));
}

#[test]
fn scan_omits_claude_auto_memory_when_absent() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".gemini")).unwrap();
    std::fs::write(home.path().join(".gemini/GEMINI.md"), "").unwrap();
    std::fs::create_dir_all(home.path().join(".claude/projects/-Users-empty/memory")).unwrap();

    let dto = scan_home(home.path());

    assert!(dto.claude_memories.is_empty());
    assert!(dto
        .findings
        .iter()
        .all(|finding| { finding.kind != crate::domain::InstructionCheckupKind::AutoMemory }));
    checkup_named(
        &dto,
        crate::domain::InstructionCheckupKind::Empty,
        "~/.gemini/GEMINI.md",
    );
}

fn cursor_state_vscdb(home: &std::path::Path) -> std::path::PathBuf {
    home.join("Library/Application Support/Cursor/User/globalStorage/state.vscdb")
}

fn write_cursor_state_vscdb(home: &std::path::Path, rows: &[(&str, &str)]) {
    let path = cursor_state_vscdb(home);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value TEXT)",
        [],
    )
    .unwrap();
    for (key, value) in rows {
        conn.execute(
            "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
            rusqlite::params![*key, *value],
        )
        .unwrap();
    }
}

fn scan_home(home: &std::path::Path) -> crate::domain::GlobalInstructionDto {
    crate::instructions::scan(
        home,
        None,
        &crate::domain::InstructionUsageSummary::default(),
    )
}

#[test]
fn scan_reports_orphan_cursor_memories_as_checkup_only() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".gemini")).unwrap();
    std::fs::write(home.path().join(".gemini/GEMINI.md"), "").unwrap();
    write_cursor_state_vscdb(
        home.path(),
        &[(
            "cursor/pendingMemories",
            r#"[{"id":"m1","memory":"fixture-alpha","title":"alpha","timestamp":1755188329776},{"id":"m2","memory":"fixture-beta","title":"beta","timestamp":1757074594123}]"#,
        )],
    );
    let db_path = cursor_state_vscdb(home.path());
    let before_bytes = std::fs::read(&db_path).unwrap();
    let before_mtime = std::fs::metadata(&db_path).unwrap().modified().unwrap();

    let dto = scan_home(home.path());

    let finding = checkup_named(
        &dto,
        crate::domain::InstructionCheckupKind::OrphanMemories,
        "cursor/pendingMemories",
    );
    assert_eq!(finding.source, "cursor");
    assert_eq!(
        finding.severity,
        crate::domain::InstructionCheckupSeverity::Medium
    );
    assert!(finding.problem.contains("残留"));
    assert!(finding.problem.contains('2'));
    assert!(finding.problem.contains("2025-08-14"));
    assert!(finding.problem.contains("2025-09-05"));
    assert!(!finding.problem.contains("正在生效"));
    assert!(!finding.consequence.contains("正在生效"));
    assert!(finding.consequence.contains("移除"));
    assert!(finding.consequence.contains("管理"));
    assert!(!finding.problem.contains("fixture-alpha"));
    assert!(!finding.consequence.contains("fixture-alpha"));
    checkup_named(
        &dto,
        crate::domain::InstructionCheckupKind::Empty,
        "~/.gemini/GEMINI.md",
    );

    let cursor = instruction_source(&dto, "cursor");
    assert_eq!(cursor.files.len(), 1);
    assert_eq!(cursor.files[0].display_path, "Cursor 账号级偏好");
    assert!(dto.sources.iter().all(|row| {
        row.files.iter().all(|file| {
            !file.display_path.contains("pendingMemories")
                && !file.content.contains("fixture-alpha")
        })
    }));
    assert_eq!(std::fs::read(&db_path).unwrap(), before_bytes);
    assert_eq!(
        std::fs::metadata(&db_path).unwrap().modified().unwrap(),
        before_mtime
    );
}

#[test]
fn scan_skips_orphan_memories_when_key_missing_and_keeps_other_findings() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".gemini")).unwrap();
    std::fs::write(home.path().join(".gemini/GEMINI.md"), "").unwrap();
    write_cursor_state_vscdb(home.path(), &[("cursor/memoriesEnabled", "true")]);

    let dto = scan_home(home.path());

    assert!(dto
        .findings
        .iter()
        .all(|finding| { finding.kind != crate::domain::InstructionCheckupKind::OrphanMemories }));
    checkup_named(
        &dto,
        crate::domain::InstructionCheckupKind::Empty,
        "~/.gemini/GEMINI.md",
    );
}

#[test]
fn scan_skips_orphan_memories_when_structure_changed() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".gemini")).unwrap();
    std::fs::write(home.path().join(".gemini/GEMINI.md"), "").unwrap();
    write_cursor_state_vscdb(
        home.path(),
        &[("cursor/pendingMemories", r#"["not-a-memory-object", 1]"#)],
    );

    let dto = scan_home(home.path());

    assert!(dto
        .findings
        .iter()
        .all(|finding| { finding.kind != crate::domain::InstructionCheckupKind::OrphanMemories }));
    checkup_named(
        &dto,
        crate::domain::InstructionCheckupKind::Empty,
        "~/.gemini/GEMINI.md",
    );
}

#[test]
fn scan_skips_orphan_memories_when_database_unreadable() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".gemini")).unwrap();
    std::fs::write(home.path().join(".gemini/GEMINI.md"), "").unwrap();
    let db_path = cursor_state_vscdb(home.path());
    std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    std::fs::write(&db_path, "not-a-sqlite-database").unwrap();

    let dto = scan_home(home.path());

    assert!(dto
        .findings
        .iter()
        .all(|finding| { finding.kind != crate::domain::InstructionCheckupKind::OrphanMemories }));
    checkup_named(
        &dto,
        crate::domain::InstructionCheckupKind::Empty,
        "~/.gemini/GEMINI.md",
    );
}

fn file_mtime(path: &std::path::Path) -> String {
    let meta = std::fs::metadata(path).unwrap();
    chrono::DateTime::<chrono::Utc>::from(meta.modified().unwrap()).to_rfc3339()
}

#[test]
fn write_user_file_replaces_content_when_mtime_matches() {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let path = home.path().join(".claude/CLAUDE.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "old\n").unwrap();
    let expected = file_mtime(&path);

    crate::user_files::write(
        home.path(),
        data.path(),
        &path,
        "new-content\n",
        Some(expected.as_str()),
    )
    .unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new-content\n");
}

#[test]
fn write_user_file_rejects_stale_mtime_and_keeps_original() {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let path = home.path().join(".claude/CLAUDE.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "keep-me\n").unwrap();

    let error = crate::user_files::write(
        home.path(),
        data.path(),
        &path,
        "stolen\n",
        Some("2000-01-01T00:00:00+00:00"),
    )
    .unwrap_err();

    assert!(error.contains("外部被修改"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep-me\n");
}

#[test]
fn write_user_file_backs_up_original_before_replace() {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let path = home.path().join(".codex/AGENTS.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "before-backup\n").unwrap();
    let expected = file_mtime(&path);

    crate::user_files::write(
        home.path(),
        data.path(),
        &path,
        "after-backup\n",
        Some(expected.as_str()),
    )
    .unwrap();

    let backups: Vec<_> = std::fs::read_dir(data.path().join("instruction-backups"))
        .unwrap()
        .flatten()
        .flat_map(|entry| std::fs::read_dir(entry.path()).into_iter().flatten())
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("bak"))
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(
        std::fs::read_to_string(&backups[0]).unwrap(),
        "before-backup\n"
    );
}

#[test]
fn write_user_file_rejects_path_outside_allowlist() {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let path = home.path().join(".codex/rules/default.rules");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "third-party\n").unwrap();

    let error = crate::user_files::write(
        home.path(),
        data.path(),
        &path,
        "nope\n",
        Some(file_mtime(&path).as_str()),
    )
    .unwrap_err();

    assert!(error.contains("可写名单"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "third-party\n");
}

#[test]
fn write_user_file_allows_grok_home_instruction() {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let path = home.path().join(".grok/AGENTS.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "old\n").unwrap();
    let expected = file_mtime(&path);

    crate::user_files::write(
        home.path(),
        data.path(),
        &path,
        "new\n",
        Some(expected.as_str()),
    )
    .unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\n");
}

#[test]
fn write_user_file_rejects_grok_config() {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let path = home.path().join(".grok/config.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "keep\n").unwrap();

    let error = crate::user_files::write(
        home.path(),
        data.path(),
        &path,
        "nope\n",
        Some(file_mtime(&path).as_str()),
    )
    .unwrap_err();

    assert!(error.contains("可写名单"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep\n");
}

#[test]
fn write_user_file_allows_grok_rules_markdown() {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let path = home.path().join(".grok/rules/style.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "old\n").unwrap();
    let expected = file_mtime(&path);

    crate::user_files::write(
        home.path(),
        data.path(),
        &path,
        "new\n",
        Some(expected.as_str()),
    )
    .unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\n");
}

#[test]
fn scan_lists_grok_rules_markdown_and_marks_editable() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".grok/rules")).unwrap();
    std::fs::write(home.path().join(".grok/AGENTS.md"), "grok-global\n").unwrap();
    std::fs::write(home.path().join(".grok/rules/style.md"), "grok-rule\n").unwrap();
    std::fs::write(home.path().join(".grok/rules/ignore.txt"), "skip\n").unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    let grok = instruction_source(&dto, "grok");
    let agents = file_named(grok, "~/.grok/AGENTS.md");
    assert!(agents.editable);
    assert_eq!(agents.content, "grok-global\n");
    let rule = file_named(grok, "~/.grok/rules/style.md");
    assert!(rule.editable);
    assert_eq!(rule.content, "grok-rule\n");
    assert!(grok
        .files
        .iter()
        .all(|file| file.display_path != "~/.grok/rules/ignore.txt"));
}

#[test]
fn scan_skips_empty_claude_rules_directory() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".claude/rules")).unwrap();
    std::fs::write(home.path().join(".claude/CLAUDE.md"), "ok\n").unwrap();

    let dto = crate::instructions::scan(
        home.path(),
        None,
        &crate::domain::InstructionUsageSummary::default(),
    );
    let claude = instruction_source(&dto, "claude");
    assert!(claude
        .files
        .iter()
        .all(|file| file.display_path != "~/.claude/rules/"));
    assert!(file_named(claude, "~/.claude/CLAUDE.md").editable);
}

#[test]
fn write_user_file_rejects_third_party_database() {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let path = home
        .path()
        .join("Library/Application Support/Cursor/User/globalStorage/state.vscdb");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "db").unwrap();

    let error = crate::user_files::write(home.path(), data.path(), &path, "x", None).unwrap_err();
    assert!(error.contains("可写名单"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "db");
}

#[test]
fn write_user_file_rejects_parent_dir_name_in_allowlist() {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let path = home.path().join(".claude/rules/../CLAUDE.md");
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::fs::write(home.path().join(".claude/CLAUDE.md"), "keep\n").unwrap();

    let error = crate::user_files::write(home.path(), data.path(), &path, "x\n", None).unwrap_err();
    assert!(error.contains("可写名单"));
    assert_eq!(
        std::fs::read_to_string(home.path().join(".claude/CLAUDE.md")).unwrap(),
        "keep\n"
    );
}

#[test]
fn open_target_uses_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("CLAUDE.md");
    std::fs::write(&path, "x\n").unwrap();
    assert_eq!(
        crate::instructions::resolve_open_path(path.to_str().unwrap()).unwrap(),
        path
    );
}

#[test]
fn open_target_uses_directory_when_path_is_dir() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rules");
    std::fs::create_dir_all(&path).unwrap();
    assert_eq!(
        crate::instructions::resolve_open_path(path.to_str().unwrap()).unwrap(),
        path
    );
}

#[test]
fn open_target_falls_back_to_parent_when_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("CLAUDE.md");
    assert_eq!(
        crate::instructions::resolve_open_path(path.to_str().unwrap()).unwrap(),
        dir.path()
    );
}

#[test]
fn open_target_rejects_empty_path() {
    let error = crate::instructions::resolve_open_path("").unwrap_err();
    assert!(error.contains("没有可打开"));
}

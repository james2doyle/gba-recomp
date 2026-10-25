//! Aceite da SPEC-070 (A-70-01..11) sobre o parser `gba-profile`.

use super::*;

const SHA: &str = "2f978f635b9593f6ca26ec42481c53a6b39f6cddd894ad5c062c1419fac58825";

fn minimal(presentation: &str, patches: &str) -> String {
    format!(
        r#"format = "gba-recomp-profile"
version = 1
id = "test-title"
status = "draft"

[image]
sha256 = "{SHA}"
size = 16
{presentation}{patches}"#
    )
}

/// A-70-01 / A-70-02: perfil mínimo carrega; `presentation` é opcional.
#[test]
fn minimal_profile_loads_without_presentation() {
    let p = parse(&minimal("", "")).unwrap();
    assert_eq!(p.id, "test-title");
    assert_eq!(p.status, ProfileStatus::Draft);
    assert_eq!(p.view_ceiling(), None, "ausente => 240");
    assert_eq!(p.presentation.margin_fill, MarginFill::Backdrop);
    assert!(!p.presentation.widen_obj);
    assert!(p.patches.is_empty());
}

/// A-70-02: um teto declarado é lido e respeita a faixa.
#[test]
fn presentation_ceiling_is_read() {
    let p = parse(&minimal("[presentation]\nview_ceiling = 288\n", "")).unwrap();
    assert_eq!(p.view_ceiling(), Some(288));
}

/// A-70-07 / PATCH-70-07: teto fora da faixa é recusado.
#[test]
fn ceiling_outside_range_is_refused() {
    for bad in ["239", "385", "1000"] {
        let text = minimal(&format!("[presentation]\nview_ceiling = {bad}\n"), "");
        assert!(
            parse(&text).is_err(),
            "view_ceiling = {bad} must be refused"
        );
    }
}

/// A-70-11: chave desconhecida é erro, em qualquer seção.
#[test]
fn unknown_keys_are_refused() {
    let cases = [
        minimal("", "").replace("id = \"test-title\"", "id = \"x\"\nbogus = 1"),
        minimal("[presentation]\nbogus = 1\n", ""),
        minimal("", "[[patch]]\nbogus = 1\n"),
        minimal("", "").replace("[image]", "[imagee]"),
    ];
    for text in cases {
        assert!(parse(&text).is_err(), "must refuse:\n{text}");
    }
}

/// PROF-01: versão diferente de 1 é recusada, e `format` é obrigatório.
#[test]
fn version_and_format_are_enforced() {
    let wrong_version = minimal("", "").replace("version = 1", "version = 2");
    assert!(parse(&wrong_version).is_err());
    let wrong_format = minimal("", "").replace("gba-recomp-profile", "other");
    assert!(parse(&wrong_format).is_err());
    let missing = minimal("", "").replace("format = \"gba-recomp-profile\"\n", "");
    assert!(parse(&missing).is_err());
}

/// PROF-01: o SHA precisa ter 64 dígitos hexadecimais.
#[test]
fn sha_must_be_sixty_four_hex_digits() {
    let short = minimal("", "").replace(SHA, "abc");
    assert!(parse(&short).is_err());
    let non_hex = minimal("", "").replace(SHA, &"z".repeat(64));
    assert!(parse(&non_hex).is_err());
}

/// PATCH-70-03: tamanhos diferentes obrigam a escrever o preenchimento.
#[test]
fn expected_and_replacement_must_match_in_length() {
    let patch = r#"
[[patch]]
offset = 0x10
expected = "01 02 03 04"
replacement = "01 02"
reason = "shorter"
evidence = "KNO-011"
affects = "framing"
"#;
    let e = parse(&minimal("", patch)).unwrap_err();
    assert!(e.0.contains("PATCH-70-03"), "{e}");
}

/// PATCH-70-06: `reason`, `evidence` e `affects` são obrigatórios.
#[test]
fn patch_metadata_is_mandatory() {
    for missing in ["reason", "evidence", "affects"] {
        let mut body = String::from(
            "\n[[patch]]\noffset = 0x10\nexpected = \"01 02\"\nreplacement = \"03 04\"\n\
             reason = \"r\"\nevidence = \"e\"\naffects = \"framing\"\n",
        );
        // Remove a linha do campo exigido.
        body = body
            .lines()
            .filter(|l| !l.trim_start().starts_with(&format!("{missing} =")))
            .collect::<Vec<_>>()
            .join("\n");
        let text = minimal("", &body);
        assert!(parse(&text).is_err(), "missing {missing} must be refused");
    }
}

/// PATCH-70-05: só bytes literais; nada de expressão ou script.
#[test]
fn patch_bytes_must_be_literal_hex() {
    let patch = r#"
[[patch]]
offset = 0x10
expected = "mov r0,#0x140"
replacement = "01 02"
reason = "r"
evidence = "e"
affects = "framing"
"#;
    assert!(
        parse(&minimal("", patch)).is_err(),
        "mnemonics are not bytes"
    );
}

/// PATCH-70-01: `expected` divergente aborta e não altera a imagem.
#[test]
fn mismatched_expected_aborts_without_mutating() {
    let mut rom = vec![0u8; 16];
    rom[..4].copy_from_slice(&[0x01, 0x02, 0x03, 0x04]);
    let before = rom.clone();
    let patch = RomPatch {
        offset: 0,
        expected: vec![0xAA, 0xBB],
        replacement: vec![0x00, 0x00],
        reason: "r".into(),
        evidence: "e".into(),
        affects: PatchScope::Framing,
    };
    match apply_patches(&mut rom, std::slice::from_ref(&patch)) {
        Err(PatchError::Mismatch { offset, .. }) => assert_eq!(offset, 0),
        other => panic!("expected Mismatch, got {other:?}"),
    }
    assert_eq!(rom, before, "a failed patch must not touch the image");
}

/// PATCH-70-01/06: patch correto aplica e descreve motivo e evidência.
#[test]
fn matching_patch_applies_and_is_described() {
    let mut rom = vec![0u8; 16];
    rom[4..6].copy_from_slice(&[0x40, 0x01]);
    let patch = RomPatch {
        offset: 4,
        expected: vec![0x40, 0x01],
        replacement: vec![0x80, 0x01],
        reason: "camera clamp 0x140 -> 0x180".into(),
        evidence: "KNO-012".into(),
        affects: PatchScope::Framing,
    };
    let applied = apply_patches(&mut rom, std::slice::from_ref(&patch)).unwrap();
    assert_eq!(rom[4..6], [0x80, 0x01]);
    assert_eq!(applied.len(), 1);
    let text = applied[0].to_string();
    assert!(text.contains("0x4"), "{text}");
    assert!(text.contains("framing"), "{text}");
    assert!(text.contains("KNO-012"), "{text}");
}

/// PATCH-70-04: offsets sobrepostos são erro de perfil.
#[test]
fn overlapping_patches_are_refused() {
    let mut rom = vec![0u8; 16];
    let mk = |offset: usize| RomPatch {
        offset,
        expected: vec![0u8; 4],
        replacement: vec![0xFFu8; 4],
        reason: "r".into(),
        evidence: "e".into(),
        affects: PatchScope::Framing,
    };
    let err = apply_patches(&mut rom, &[mk(0), mk(2)]).unwrap_err();
    assert!(matches!(err, PatchError::Overlap { .. }), "{err:?}");
    // Disjuntos passam.
    assert!(apply_patches(&mut rom, &[mk(0), mk(4)]).is_ok());
}

/// Patch além do fim da imagem é recusado, não truncado.
#[test]
fn patch_past_end_of_image_is_refused() {
    let mut rom = vec![0u8; 8];
    let patch = RomPatch {
        offset: 6,
        expected: vec![0u8; 4],
        replacement: vec![0xFFu8; 4],
        reason: "r".into(),
        evidence: "e".into(),
        affects: PatchScope::Culling,
    };
    assert!(matches!(
        apply_patches(&mut rom, std::slice::from_ref(&patch)),
        Err(PatchError::OutOfRange { .. })
    ));
}

/// A-70-04 / PROF-02: só o SHA exato autoriza; revisão vizinha não.
#[test]
fn only_the_exact_hash_matches() {
    let p = parse(&minimal("", "")).unwrap();
    assert!(p.matches_image(SHA, 16));
    assert!(
        p.matches_image(&SHA.to_ascii_uppercase(), 16),
        "case-insensitive"
    );
    assert!(!p.matches_image(&"0".repeat(64), 16), "different hash");
    assert!(!p.matches_image(SHA, 32), "size mismatch");
    let msg = hash_mismatch_message(&p, &"0".repeat(64));
    assert!(msg.contains("ignoring"), "{msg}");
}

/// `margin_fill` e `affects` só aceitam os valores declarados.
#[test]
fn enums_are_closed() {
    assert!(parse(&minimal("[presentation]\nmargin_fill = \"grey\"\n", "")).is_err());
    assert!(parse(&minimal("[presentation]\nmargin_fill = \"black\"\n", "")).is_ok());
    let bad_scope = r#"
[[patch]]
offset = 0x0
expected = "00"
replacement = "01"
reason = "r"
evidence = "e"
affects = "magic"
"#;
    assert!(parse(&minimal("", bad_scope)).is_err());
}

/// Seções do SPEC-008 que esta v1 reconhece não são erro.
#[test]
fn known_spec008_tables_are_tolerated() {
    let text = minimal("", "") + "\n[analysis]\nmax_blocks = 10\n\n[runtime]\nbackup = \"none\"\n";
    assert!(parse(&text).is_ok(), "known tables must not be rejected");
}

/// A config real de leafgreen, já no repositório, continua válida.
#[test]
fn repository_profile_parses() {
    let text = include_str!("../../../profiles/leafgreen-bpge-rev1.toml");
    let p = parse(text).expect("the checked-in profile must parse");
    assert_eq!(p.id, "leafgreen-bpge-rev1");
    assert_eq!(p.view_ceiling(), None, "no presentation section yet");
    assert!(p.patches.is_empty());
}

/// A tolerância a tabelas conhecidas não pode abrir a porta para qualquer
/// coisa: uma tabela desconhecida de primeiro nível continua sendo erro.
#[test]
fn tolerance_is_narrow() {
    let bad = minimal("", "") + "\n[whatever]\nx = 1\n";
    assert!(parse(&bad).is_err(), "unknown root table must be refused");
    let bad_nested = minimal("", "") + "\n[whatever.deep]\nx = 1\n";
    assert!(
        parse(&bad_nested).is_err(),
        "unknown nested root must be refused"
    );
    // E `presentation` (a nossa) continua estrita.
    let unknown_key = minimal("[presentation]\nview_width = 288\n", "");
    assert!(
        parse(&unknown_key).is_err(),
        "only view_ceiling is accepted"
    );
}

/// `#` dentro de aspas é valor, não comentário (exemplo da própria SPEC-070).
#[test]
fn quoted_hash_survives_comment_stripping() {
    let patch = r#"
[[patch]]
offset = 0x10
expected = "01 20"
replacement = "00 20"
reason = "camera clamp derecho: 0x140 -> 0x180" # trailing comment
evidence = "KNO-012#camera-clamp"
affects = "framing"
"#;
    let p = parse(&minimal(
        "[presentation]\nevidence = \"KNO-011 WS-32, ring 64 / authored 16\" # nota\n",
        patch,
    ))
    .unwrap();
    assert_eq!(p.patches[0].evidence, "KNO-012#camera-clamp");
    assert_eq!(p.patches[0].reason, "camera clamp derecho: 0x140 -> 0x180");
    assert_eq!(
        p.presentation.evidence.as_deref(),
        Some("KNO-011 WS-32, ring 64 / authored 16")
    );
}

/// A API pública não pode entrar em pânico com `expected`/`replacement` de
/// tamanhos diferentes, mesmo que o parser sempre confira (PATCH-70-03).
#[test]
fn apply_patches_rejects_length_mismatch() {
    let mut rom = vec![0u8; 16];
    let patch = RomPatch {
        offset: 4,
        expected: vec![0x40, 0x01],
        replacement: vec![0x80],
        reason: "r".into(),
        evidence: "e".into(),
        affects: PatchScope::Framing,
    };
    match apply_patches(&mut rom, std::slice::from_ref(&patch)) {
        Err(PatchError::Length {
            expected_len: 2,
            replacement_len: 1,
            ..
        }) => {}
        other => panic!("expected Length, got {other:?}"),
    }
}

/// Offset que estoura `offset + len` é erro alto, não pânico.
#[test]
fn apply_patches_rejects_overflowing_offset() {
    let mut rom = vec![0u8; 16];
    let patch = RomPatch {
        offset: usize::MAX - 1,
        expected: vec![0x00, 0x00, 0x00, 0x00],
        replacement: vec![0x01, 0x02, 0x03, 0x04],
        reason: "r".into(),
        evidence: "e".into(),
        affects: PatchScope::Framing,
    };
    match apply_patches(&mut rom, std::slice::from_ref(&patch)) {
        Err(PatchError::OutOfRange { offset, .. }) => assert_eq!(offset, usize::MAX - 1),
        other => panic!("expected OutOfRange, got {other:?}"),
    }
    assert_eq!(rom, vec![0u8; 16], "the image must stay untouched");
}

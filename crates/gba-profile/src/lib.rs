//! Perfil de jogo v1 (SPEC-008) com a extensão `presentation` e `[[patch]]`
//! de [SPEC-070](../../../specs/SPEC-070-rom-only-widescreen-migration.md).
//!
//! O parser é estrito de propósito: chave ou versão desconhecida é erro, o
//! SHA-256 é obrigatório, e nenhum campo técnico é aplicado antes do hash
//! bater (PROF-01, PROF-02, PATCH-70-02). Não há scripts: `[[patch]]` são
//! bytes literais (PROF-03, PATCH-70-05).
//!
//! Formato TOML UTF-8, `format = "gba-recomp-profile"`, `version = 1`.

use sha2::{Digest, Sha256};

/// Faixa aceita de `view_ceiling` (SPEC-069 WS-69-05).
pub const VIEW_WIDTH_RANGE: (u16, u16) = (240, 384);

/// Preenchimento de margem recusada para este título (SPEC-070).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MarginFill {
    /// `palette[0]`: o que o hardware mostraria. Padrão fiel.
    #[default]
    Backdrop,
    Black,
}

impl MarginFill {
    pub fn as_str(self) -> &'static str {
        match self {
            MarginFill::Backdrop => "backdrop",
            MarginFill::Black => "black",
        }
    }

    fn parse(text: &str) -> Result<Self, String> {
        match text {
            "backdrop" => Ok(MarginFill::Backdrop),
            "black" => Ok(MarginFill::Black),
            other => Err(format!(
                "`margin_fill` must be \"backdrop\" or \"black\", got {other:?}"
            )),
        }
    }
}

/// O que um patch pretende corrigir. Serve de auditoria e de recorte de teste.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchScope {
    Framing,
    Hud,
    Culling,
}

impl PatchScope {
    pub fn as_str(self) -> &'static str {
        match self {
            PatchScope::Framing => "framing",
            PatchScope::Hud => "hud",
            PatchScope::Culling => "culling",
        }
    }

    fn parse(text: &str) -> Result<Self, String> {
        match text {
            "framing" => Ok(PatchScope::Framing),
            "hud" => Ok(PatchScope::Hud),
            "culling" => Ok(PatchScope::Culling),
            other => Err(format!(
                "`affects` must be framing, hud or culling, got {other:?}"
            )),
        }
    }
}

/// Seção `[presentation]` (SPEC-070).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Presentation {
    /// Largura máxima que o título tolera; alimenta WS-69-19.
    pub view_ceiling: Option<u16>,
    pub margin_fill: MarginFill,
    /// Desenhar OBJ fora das 240 colunas (WS-69-10). Padrão `false`.
    pub widen_obj: bool,
    pub evidence: Option<String>,
}

/// Um patch de bytes, verificado antes de ser aplicado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RomPatch {
    pub offset: usize,
    pub expected: Vec<u8>,
    pub replacement: Vec<u8>,
    pub reason: String,
    pub evidence: String,
    pub affects: PatchScope,
}

impl RomPatch {
    fn end(&self) -> Option<usize> {
        self.offset.checked_add(self.expected.len())
    }
}

/// Perfil resolvido para uma imagem concreta.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub id: String,
    pub status: ProfileStatus,
    pub rom_sha256: String,
    pub rom_size: Option<u64>,
    pub presentation: Presentation,
    pub patches: Vec<RomPatch>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileStatus {
    Draft,
    Validated,
}

impl ProfileStatus {
    fn parse(text: &str) -> Result<Self, String> {
        match text {
            "draft" => Ok(ProfileStatus::Draft),
            "validated" => Ok(ProfileStatus::Validated),
            other => Err(format!(
                "`status` must be \"draft\" or \"validated\", got {other:?}"
            )),
        }
    }
}

/// Falha de aplicação de patch. Nunca é parcial: ou todos aplicam, ou nada.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchError {
    /// `expected` não confere com a imagem carregada (PATCH-70-01).
    Mismatch {
        offset: usize,
        expected: Vec<u8>,
        found: Vec<u8>,
    },
    /// Patch alcança região fora da imagem.
    OutOfRange {
        offset: usize,
        len: usize,
        rom_len: usize,
    },
    /// Dois patches se sobrepõem (PATCH-70-04).
    Overlap { first: usize, second: usize },
    /// `expected` e `replacement` têm tamanhos diferentes (PATCH-70-03).
    Length {
        offset: usize,
        expected_len: usize,
        replacement_len: usize,
    },
}

impl std::fmt::Display for PatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatchError::Mismatch {
                offset,
                expected,
                found,
            } => write!(
                f,
                "patch at 0x{offset:X}: expected {} but found {}",
                hex(expected),
                hex(found)
            ),
            PatchError::OutOfRange {
                offset,
                len,
                rom_len,
            } => write!(
                f,
                "patch at 0x{offset:X} (+{len} bytes) exceeds the ROM ({rom_len} bytes)"
            ),
            PatchError::Overlap { first, second } => {
                write!(f, "patches at 0x{first:X} and 0x{second:X} overlap")
            }
            PatchError::Length {
                offset,
                expected_len,
                replacement_len,
            } => write!(
                f,
                "patch at 0x{offset:X}: expected is {expected_len} bytes but replacement is \
                 {replacement_len} (PATCH-70-03)"
            ),
        }
    }
}

impl std::error::Error for PatchError {}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&format!("{b:02X}"));
    }
    s
}

/// Descreve um patch aplicado, para o log de execução (PATCH-70-06).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedPatch {
    pub offset: usize,
    pub reason: String,
    pub evidence: String,
    pub affects: PatchScope,
}

impl std::fmt::Display for AppliedPatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "patch 0x{:X} [{}] {} (evidence: {})",
            self.offset,
            self.affects.as_str(),
            self.reason,
            self.evidence
        )
    }
}

/// Aplica patches verificados a uma imagem já validada por SHA-256.
///
/// Verifica todos antes de escrever qualquer byte: uma falha não deixa a
/// imagem meio alterada (PATCH-70-01).
pub fn apply_patches(
    rom: &mut [u8],
    patches: &[RomPatch],
) -> Result<Vec<AppliedPatch>, PatchError> {
    let mut ends = Vec::with_capacity(patches.len());
    for patch in patches {
        if patch.replacement.len() != patch.expected.len() {
            return Err(PatchError::Length {
                offset: patch.offset,
                expected_len: patch.expected.len(),
                replacement_len: patch.replacement.len(),
            });
        }
        ends.push(patch.end().ok_or(PatchError::OutOfRange {
            offset: patch.offset,
            len: patch.expected.len(),
            rom_len: rom.len(),
        })?);
    }
    // Sobreposição é erro de perfil, não "último ganha" (PATCH-70-04).
    let mut order: Vec<usize> = (0..patches.len()).collect();
    order.sort_unstable_by_key(|&i| (patches[i].offset, ends[i]));
    for pair in order.windows(2) {
        if patches[pair[1]].offset < ends[pair[0]] {
            return Err(PatchError::Overlap {
                first: patches[pair[0]].offset,
                second: patches[pair[1]].offset,
            });
        }
    }
    // Verifica tudo primeiro.
    for (patch, &end) in patches.iter().zip(&ends) {
        if end > rom.len() {
            return Err(PatchError::OutOfRange {
                offset: patch.offset,
                len: patch.expected.len(),
                rom_len: rom.len(),
            });
        }
        let found = &rom[patch.offset..end];
        if found != patch.expected.as_slice() {
            return Err(PatchError::Mismatch {
                offset: patch.offset,
                expected: patch.expected.clone(),
                found: found.to_vec(),
            });
        }
    }
    // Só então escreve.
    let mut applied = Vec::with_capacity(patches.len());
    for (patch, &end) in patches.iter().zip(&ends) {
        rom[patch.offset..end].copy_from_slice(&patch.replacement);
        applied.push(AppliedPatch {
            offset: patch.offset,
            reason: patch.reason.clone(),
            evidence: patch.evidence.clone(),
            affects: patch.affects,
        });
    }
    Ok(applied)
}

/// SHA-256 hexadecimal minúsculo de uma imagem.
pub fn rom_sha256(rom: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(rom);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Emite a mensagem de diagnóstico para um perfil que não corresponde à
/// imagem. Nunca é erro fatal: o jogo roda em 240 (A-70-04).
pub fn hash_mismatch_message(profile: &Profile, actual: &str) -> String {
    format!(
        "profile {} targets {}, but this image is {}; ignoring its presentation and patches",
        profile.id, profile.rom_sha256, actual
    )
}

impl Profile {
    /// Confere se o perfil se aplica a esta imagem (PROF-02).
    pub fn matches_image(&self, rom_sha256: &str, rom_len: usize) -> bool {
        self.rom_sha256.eq_ignore_ascii_case(rom_sha256)
            && self.rom_size.is_none_or(|s| s == rom_len as u64)
    }

    /// Teto efetivo do perfil, se houver.
    pub fn view_ceiling(&self) -> Option<u16> {
        self.presentation.view_ceiling
    }
}

// ------------------------------------------------------------------- parser

/// Erros do parser. Todos impedem o uso do perfil (PROF-01).
#[derive(Debug, PartialEq, Eq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParseError {}

impl From<String> for ParseError {
    fn from(msg: String) -> Self {
        ParseError(msg)
    }
}

fn err<T>(msg: impl Into<String>) -> Result<T, ParseError> {
    Err(ParseError(msg.into()))
}

/// Lê uma string TOML simples, com os escapes aceitos pelo projeto.
fn strip_comment(line: &str) -> &str {
    let mut in_str = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '#' if !in_str => return &line[..i],
            _ => {}
        }
    }
    line
}

fn unquote(text: &str) -> Result<String, ParseError> {
    let t = text.trim();
    let inner = t
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| ParseError(format!("expected a quoted string, got {t:?}")))?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                _ => return err("unsupported escape in string"),
            },
            c => out.push(c),
        }
    }
    Ok(out)
}

fn parse_u64(text: &str, what: &str) -> Result<u64, ParseError> {
    let t = text.trim();
    let parsed = if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16)
    } else {
        t.parse::<u64>()
    };
    parsed.map_err(|_| ParseError(format!("`{what}` must be an integer, got {t:?}")))
}

/// Bytes em hexadecimal separados por espaço (`"01 20 08 40"`).
fn parse_bytes(text: &str, what: &str) -> Result<Vec<u8>, ParseError> {
    let inner = unquote(text)?;
    let mut out = Vec::new();
    for token in inner.split_whitespace() {
        if token.len() != 2 {
            return err(format!("`{what}`: {token:?} is not a two-digit hex byte"));
        }
        let byte = u8::from_str_radix(token, 16)
            .map_err(|_| ParseError(format!("`{what}`: {token:?} is not hex")))?;
        out.push(byte);
    }
    if out.is_empty() {
        return err(format!("`{what}` must not be empty"));
    }
    Ok(out)
}

/// Estado de seção durante a varredura.
enum Section {
    Root,
    Image,
    Presentation,
    Patch(usize),
    /// Seções do SPEC-008 que esta v1 reconhece e ignora de propósito.
    Ignored,
}

/// Faz o parse de um perfil v1.
pub fn parse(text: &str) -> Result<Profile, ParseError> {
    let mut format_seen = false;
    let mut version_seen = false;
    let mut id: Option<String> = None;
    let mut status: Option<ProfileStatus> = None;
    let mut rom_sha256: Option<String> = None;
    let mut rom_size: Option<u64> = None;
    let mut presentation = Presentation::default();
    let mut patches: Vec<PatchDraft> = Vec::new();
    let mut section = Section::Root;

    for (lineno, raw) in text.lines().enumerate() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        // `[[patch]]` é array-of-tables; `[analysis.seeds]` é tabela aninhada.
        if let Some(rest) = line.strip_prefix("[[") {
            let header = rest
                .strip_suffix("]]")
                .ok_or_else(|| ParseError(format!("line {}: unterminated table", lineno + 1)))?;
            if header.starts_with("analysis.")
                || header.starts_with("overlays.")
                || header.starts_with("runtime.")
                || header.starts_with("compiler.")
            {
                section = Section::Ignored;
                continue;
            }
            section = match header {
                "patch" => {
                    patches.push(PatchDraft::default());
                    Section::Patch(patches.len() - 1)
                }
                other => {
                    return err(format!(
                        "line {}: unknown array table [[{other}]] (SPEC-008 PROF-01)",
                        lineno + 1
                    ));
                }
            };
            continue;
        }
        if let Some(header) = line.strip_prefix('[') {
            let header = header
                .strip_suffix(']')
                .ok_or_else(|| ParseError(format!("line {}: unterminated table", lineno + 1)))?;
            // Tabelas aninhadas de seções conhecidas do SPEC-008 são aceitas e
            // ignoradas nesta v1 (o perfil completo é validado por SPEC-008).
            let known_root = |root: &str| {
                matches!(
                    root,
                    "analysis"
                        | "overlays"
                        | "runtime"
                        | "compiler"
                        | "evidence"
                        | "presentation_defaults"
                )
            };
            if let Some((root, _)) = header.split_once('.')
                && known_root(root)
            {
                section = Section::Ignored;
                continue;
            }
            // Tabela de preferências globais do SPEC-008: aceita e ignorada
            // (o teto por título vive em [presentation], não aqui).
            if known_root(header) {
                section = Section::Ignored;
                continue;
            }
            section = match header {
                "image" => Section::Image,
                "presentation" => Section::Presentation,
                "analysis"
                | "overlays"
                | "runtime"
                | "compiler"
                | "evidence"
                | "presentation_defaults" => Section::Ignored,
                "patch" => {
                    patches.push(PatchDraft::default());
                    Section::Patch(patches.len() - 1)
                }
                other => {
                    return err(format!(
                        "line {}: unknown table [{other}] (SPEC-008 PROF-01)",
                        lineno + 1
                    ));
                }
            };
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| ParseError(format!("line {}: expected key = value", lineno + 1)))?;
        let (key, value) = (key.trim(), value.trim());

        match &section {
            Section::Root => match key {
                "format" => {
                    let v = unquote(value)?;
                    if v != "gba-recomp-profile" {
                        return err(format!(
                            "line {}: format must be \"gba-recomp-profile\", got {v:?}",
                            lineno + 1
                        ));
                    }
                    format_seen = true;
                }
                "version" => {
                    let v = parse_u64(value, "version")?;
                    if v != 1 {
                        return err(format!(
                            "line {}: unsupported profile version {v} (only 1)",
                            lineno + 1
                        ));
                    }
                    version_seen = true;
                }
                "id" => id = Some(unquote(value)?),
                "status" => status = Some(ProfileStatus::parse(&unquote(value)?)?),
                other => {
                    return err(format!(
                        "line {}: unknown key `{other}` (SPEC-008 PROF-01)",
                        lineno + 1
                    ));
                }
            },
            Section::Image => match key {
                "sha256" => rom_sha256 = Some(unquote(value)?.to_ascii_lowercase()),
                "size" => rom_size = Some(parse_u64(value, "size")?),
                // Descritivos: aceitos e não aplicados.
                "game_code" | "revision" => {}
                other => {
                    return err(format!(
                        "line {}: unknown key in [image]: `{other}`",
                        lineno + 1
                    ));
                }
            },
            Section::Presentation => match key {
                "view_ceiling" => {
                    let w = parse_u64(value, "view_ceiling")?;
                    let (lo, hi) = VIEW_WIDTH_RANGE;
                    if w < u64::from(lo) || w > u64::from(hi) {
                        return err(format!(
                            "line {}: view_ceiling {w} outside {lo}..={hi} (WS-69-05)",
                            lineno + 1
                        ));
                    }
                    presentation.view_ceiling = Some(w as u16);
                }
                "margin_fill" => presentation.margin_fill = MarginFill::parse(&unquote(value)?)?,
                "widen_obj" => {
                    presentation.widen_obj = match value {
                        "true" => true,
                        "false" => false,
                        other => {
                            return err(format!(
                                "line {}: widen_obj must be true or false, got {other:?}",
                                lineno + 1
                            ));
                        }
                    };
                }
                "evidence" => presentation.evidence = Some(unquote(value)?),
                other => {
                    return err(format!(
                        "line {}: unknown key in [presentation]: `{other}`",
                        lineno + 1
                    ));
                }
            },
            Section::Patch(index) => {
                let draft = &mut patches[*index];
                match key {
                    "offset" => {
                        let v = parse_u64(value, "offset")?;
                        draft.offset = Some(usize::try_from(v).map_err(|_| {
                            ParseError(format!(
                                "line {}: offset {v} does not fit usize",
                                lineno + 1
                            ))
                        })?);
                    }
                    "expected" => draft.expected = Some(parse_bytes(value, "expected")?),
                    "replacement" => draft.replacement = Some(parse_bytes(value, "replacement")?),
                    "reason" => draft.reason = Some(unquote(value)?),
                    "evidence" => draft.evidence = Some(unquote(value)?),
                    "affects" => draft.affects = Some(PatchScope::parse(&unquote(value)?)?),
                    other => {
                        return err(format!(
                            "line {}: unknown key in [[patch]]: `{other}` (no scripts, PATCH-70-05)",
                            lineno + 1
                        ));
                    }
                }
            }
            Section::Ignored => {}
        }
    }

    if !format_seen {
        return err("missing `format = \"gba-recomp-profile\"`");
    }
    if !version_seen {
        return err("missing `version = 1`");
    }
    let id = id.ok_or_else(|| ParseError("missing `id`".into()))?;
    let status = status.ok_or_else(|| ParseError("missing `status`".into()))?;
    let rom_sha256 = rom_sha256.ok_or_else(|| ParseError("missing `[image].sha256`".into()))?;
    if rom_sha256.len() != 64 || !rom_sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return err(format!(
            "[image].sha256 must be 64 hex digits, got {rom_sha256:?}"
        ));
    }

    let patches = patches
        .into_iter()
        .enumerate()
        .map(|(i, d)| d.finish(i + 1))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Profile {
        id,
        status,
        rom_sha256,
        rom_size,
        presentation,
        patches,
    })
}

#[derive(Default)]
struct PatchDraft {
    offset: Option<usize>,
    expected: Option<Vec<u8>>,
    replacement: Option<Vec<u8>>,
    reason: Option<String>,
    evidence: Option<String>,
    affects: Option<PatchScope>,
}

impl PatchDraft {
    fn finish(self, ordinal: usize) -> Result<RomPatch, ParseError> {
        let offset = self
            .offset
            .ok_or_else(|| ParseError(format!("patch #{ordinal}: missing `offset`")))?;
        let expected = self
            .expected
            .ok_or_else(|| ParseError(format!("patch #{ordinal}: missing `expected`")))?;
        let replacement = self
            .replacement
            .ok_or_else(|| ParseError(format!("patch #{ordinal}: missing `replacement`")))?;
        if expected.len() != replacement.len() {
            return err(format!(
                "patch #{ordinal}: `expected` is {} bytes but `replacement` is {}; write the \
                 padding explicitly (PATCH-70-03)",
                expected.len(),
                replacement.len()
            ));
        }
        let reason = self
            .reason
            .filter(|r| !r.trim().is_empty())
            .ok_or_else(|| ParseError(format!("patch #{ordinal}: missing `reason`")))?;
        let evidence = self
            .evidence
            .filter(|r| !r.trim().is_empty())
            .ok_or_else(|| ParseError(format!("patch #{ordinal}: missing `evidence`")))?;
        let affects = self
            .affects
            .ok_or_else(|| ParseError(format!("patch #{ordinal}: missing `affects`")))?;
        Ok(RomPatch {
            offset,
            expected,
            replacement,
            reason,
            evidence,
            affects,
        })
    }
}

// -------------------------------------------------------------- discovery

use std::path::PathBuf;

/// Diretórios onde perfis são procurados, em ordem de precedência.
///
/// `GBA_PROFILE_DIR` vence o diretório de configuração do usuário. Não há
/// leitura da árvore do repositório: o perfil precisa estar instalado.
pub fn profile_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("GBA_PROFILE_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    if let Some(cfg) = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    {
        dirs.push(cfg.join("gba-recomp").join("profiles"));
    }
    dirs
}

/// Perfil instalado que corresponde exatamente a esta imagem (PROF-02).
///
/// Perfil de outra revisão é ignorado; um perfil inválido é reportado e
/// ignorado, nunca aplicado pela metade.
pub fn find_profile(rom_sha256: &str, rom_len: usize) -> Option<Profile> {
    for dir in profile_dirs() {
        if let Some(profile) = find_profile_in(&dir, rom_sha256, rom_len) {
            return Some(profile);
        }
    }
    None
}

/// Como [`find_profile`], mas restrito a um diretório (testes e ferramentas).
pub fn find_profile_in(dir: &std::path::Path, rom_sha256: &str, rom_len: usize) -> Option<Profile> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        match parse(&text) {
            Ok(profile) if profile.matches_image(rom_sha256, rom_len) => return Some(profile),
            Ok(_) => {}
            Err(e) => eprintln!("gba-recomp: ignoring {}: {e}", path.display()),
        }
    }
    None
}

/// Resultado de preparar uma imagem: teto declarado e patches aplicados.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PreparedRom {
    pub profile_id: Option<String>,
    pub view_ceiling: Option<u16>,
    pub applied: Vec<AppliedPatch>,
}

/// Descobre o perfil desta imagem e aplica seus patches verificados.
///
/// Aplica **antes** de qualquer máquina ser construída. Um `expected` que não
/// confere retorna `Err` e deixa a imagem intacta (PATCH-70-01).
pub fn prepare_rom(rom: &mut [u8]) -> Result<PreparedRom, String> {
    let sha = rom_sha256(rom);
    let Some(profile) = find_profile(&sha, rom.len()) else {
        return Ok(PreparedRom::default());
    };
    let applied =
        apply_patches(rom, &profile.patches).map_err(|e| format!("profile {}: {e}", profile.id))?;
    let ceiling = profile.view_ceiling();
    Ok(PreparedRom {
        profile_id: Some(profile.id),
        view_ceiling: ceiling,
        applied,
    })
}

#[cfg(test)]
mod tests;

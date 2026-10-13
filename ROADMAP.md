# Roadmap

Fonte única de andamento. Data: 2026-10-13. `DONE` exige evidência; `BACKLOG` ainda não começou; `IN_PROGRESS` indica trabalho iniciado, com gates pendentes. Estados Draft/Review das specs indicam revisão documental; aceite de ADR e seleção de toolchain não significam implementação executável.

| ID | Prioridade | Entrega | Estado | Dependências | Contrato/evidência |
|---|---|---|---|---|---|
| GBA-001 | P0 | Especificação SDD inicial, referência e inventário | DONE | — | README, 13 specs, ADR-001–003, KNO-001/002/004, clone mGBA fixado por commit; revisão documental local |
| GBA-005 | P0 | Revisar contratos e fechar versões/toolchains | DONE | GBA-001 | ADR-004, `toolchain.lock.toml`, SPEC-004/010 em Review, KNO-001/003; hashes upstream registrados |
| GBA-010 | P0 | Scaffold, ABI, GBA IR mínima e LLVM objeto/link | IN_PROGRESS | GBA-005 | Fixture O0/O2 emitida como Mach-O, linkada e executada em macOS arm64; validação Linux x86-64 ainda pendente |
| GBA-020 | P0 | Intake, decoder e primeiro homebrew headless | IN_PROGRESS | GBA-010 (gate Linux x86-64 ainda pendente) | SPEC-012; Tiny Counter, 6 instruções estritamente AOT, O0/O2 executados em macOS arm64; LeafGreen bloqueada por opcode ainda não suportado |
| GBA-025 | P0 | CFG, ARM/Thumb, dispatcher, guards e overlays | IN_PROGRESS | GBA-020 (LeafGreen ainda falha no decoder) | SPEC-003/013; primeira extensão de CPU para bancos ARM e MSR CPSR |
| GBA-030 | P0 | Bus, timing, DMA, timers e IRQ | BACKLOG | GBA-020 | SPEC-005; conformance temporal |
| GBA-040 | P0 | PPU software e APU canônica | BACKLOG | GBA-030 | SPEC-006; gráfico e áudio homebrew |
| GBA-045 | P0 | HLE mínimo, backups e estado | BACKLOG | GBA-030 | SPEC-007; save/restore e SWIs da fixture |
| GBA-050 | P0 | Driver AOT, passes O0/O2 e cache | BACKLOG | GBA-025, GBA-030 | SPEC-004; mesma execução em ambos os pipelines |
| GBA-060 | P0 | Parser/perfis, hints e digests | BACKLOG | GBA-025, GBA-050 | SPEC-008; exemplos e rejeições |
| GBA-070 | P0 | Frontend/menu, input e configurações | BACKLOG | GBA-040, GBA-045, GBA-060 | SPEC-009; fluxo sem mouse e hotplug |
| GBA-080 | P0 | Pacotes Linux x86-64/macOS arm64 | BACKLOG | GBA-070 | SPEC-010/011; máquina sem LLVM |
| GBA-090 | P1 | Qualificar cenário LeafGreen BPGE revisão 1 | BACKLOG | GBA-080 | KNO-004, perfil draft; boot/input/save e cobertura medida |
| GBA-095 | P1 | Windows x86-64/Linux arm64 | BACKLOG | GBA-080 | SPEC-010; adaptadores e matriz própria |
| GBA-100 | P2 | Mobile/Web, link e cartuchos especiais | BACKLOG | GBA-090 | Specs incrementais e corpus selecionado |

## Gates

G0: contrato revisado e toolchains fixadas. G1: objeto nativo executa ALU guest. G2: homebrew produz vídeo/áudio/input/save. G3: pacote e menu portáteis. G4: cenário de jogo documentado, com rótulo strict-aot ou aot-compatible correto.

O runtime de hardware ainda não existe. GBA-005 fecha o baseline documental. Em GBA-010, a fixture O0/O2 passou novamente em macOS arm64 nativo; Linux x86-64 segue pendente porque o Docker local é arm64 e a imagem x86-64 exigiu autenticação do registry. Em GBA-020, a homebrew sintética Tiny Counter foi ingerida, seis instruções alcançáveis foram emitidas como LLVM e os executáveis host O0/O2 terminaram com estado idêntico após 100 chamadas nativas. Ela não usa memória ou hardware GBA. A compilação da ROM LeafGreen foi tentada e parou em `0x08000208` (`0xE129F000`), rejeitada pelo subconjunto atual; não foi criado pacote e o jogo não foi executado. A evidência local e hashes estão em [gba020-macos-arm64.json](knowledge/evidence/gba020-macos-arm64.json). REA já havia recusado o formato raw da ROM; na retomada, a chamada de estado do MCP falhou com `Transport closed`, sem Evidence novo.

## Verificação registrada

Em 2026-10-13: 94 links Markdown relativos sem destinos ausentes, 5 arquivos TOML e 3 JSON de evidência foram validados; `cargo fmt --all -- --check`, `cargo metadata --format-version 1 --locked --no-deps`, `cargo check --workspace --locked`, `cargo test -p gba-ir --locked` (4 testes), `cargo test -p gba-recomp --locked` (7 testes), `sh -n` dos dois scripts e `scripts/verify-gba020.sh` passaram. A verificação GBA-020 executou os pacotes O0/O2 e rejeitou checksum inválido, opcode fora do subconjunto e branch fora da ROM sem pacote parcial. `scripts/verify-gba010.sh` também passou em macOS arm64, incluindo ABI LLVM/C, link e harness; o linker Apple avisou que o objeto relocável não tem load command de plataforma e assumiu macOS. Ambos os pacotes Tiny Counter mostram apenas `libSystem` em `otool -L`. Linux x86-64 não foi executado.

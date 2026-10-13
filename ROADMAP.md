# Roadmap

Fonte única de andamento. Data: 2026-10-13. `DONE` exige evidência; `BACKLOG` ainda não começou; `IN_PROGRESS` indica trabalho iniciado, com gates pendentes. Estados Draft/Review das specs indicam revisão documental; aceite de ADR e seleção de toolchain não significam implementação executável.

| ID | Prioridade | Entrega | Estado | Dependências | Contrato/evidência |
|---|---|---|---|---|---|
| GBA-001 | P0 | Especificação SDD inicial, referência e inventário | DONE | — | README, 11 specs, ADR-001–003, KNO-001/002/004, clone mGBA fixado por commit; revisão documental local |
| GBA-005 | P0 | Revisar contratos e fechar versões/toolchains | DONE | GBA-001 | ADR-004, `toolchain.lock.toml`, SPEC-004/010 em Review, KNO-001/003; hashes upstream registrados |
| GBA-010 | P0 | Scaffold, ABI, GBA IR mínima e LLVM objeto/link | IN_PROGRESS | GBA-005 | Fixture O0/O2 emitida como Mach-O, linkada e executada em macOS arm64; validação Linux x86-64 ainda pendente |
| GBA-020 | P0 | Intake, decoder e primeiro homebrew headless | IN_PROGRESS | GBA-010 (gate Linux x86-64 ainda pendente) | SPEC-001/002/011/012 |
| GBA-025 | P0 | CFG, ARM/Thumb, dispatcher, guards e overlays | BACKLOG | GBA-020 | SPEC-003; miss estrito e fallback mensurado |
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

O runtime de hardware ainda não existe e nenhuma ROM de jogo foi executada. GBA-005 fecha o baseline documental. Em GBA-010, a fixture O0/O2 já passou em macOS arm64 nativo: objetos Mach-O, link com o harness C e casos de sucesso/falha executados. Linux x86-64 segue pendente; o Docker local é arm64 e a imagem x86-64 não pôde ser obtida por falha de autenticação do registry. GBA-020 começou com SPEC-012, que restringe o primeiro homebrew a CPU ARM headless; ainda não há execução da fixture nem cobertura da ROM LeafGreen. A inspeção anterior da ROM comercial foi limitada a metadados, hash e assinatura de backup; REA recusou o formato raw. Na retomada desta tarefa, o transporte do MCP REA está fechado; nenhuma nova evidência REA foi produzida. A inspeção documental não substitui os gates de execução das specs.

## Verificação registrada

Em 2026-10-13: 83 links Markdown relativos inspecionados (zero destinos ausentes), TOML de profiles/workspace/toolchain e JSON de evidência analisados; `cargo fmt --all -- --check`, `cargo metadata --locked`, `cargo check --workspace --locked` e `sh -n scripts/verify-gba010.sh` passaram. O SHA-256 dos pacotes LLVM Linux/macOS e `llvm-sys` confere com o lock. `scripts/verify-gba010.sh` passou em macOS arm64 nativo para O0/O2, incluindo emissão Mach-O, ABI LLVM/C, link e execução do harness; o linker Apple avisou que o objeto relocável não tem load command de plataforma e assumiu macOS. Linux x86-64 não foi executado.

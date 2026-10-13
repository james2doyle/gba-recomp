# Roadmap

Fonte única de andamento. Data: 2026-10-13. `DONE` exige evidência; `BACKLOG` ainda não começou; `IN_PROGRESS` indica trabalho iniciado, com gates pendentes. Estados Draft/Review das specs indicam revisão documental; aceite de ADR e seleção de toolchain não significam implementação executável.

| ID | Prioridade | Entrega | Estado | Dependências | Contrato/evidência |
|---|---|---|---|---|---|
| GBA-001 | P0 | Especificação SDD inicial, referência e inventário | DONE | — | README, 16 specs, ADR-001–003, KNO-001/002/004/005, ambos vendors fixados por commit; revisão documental local |
| GBA-005 | P0 | Revisar contratos e fechar versões/toolchains | DONE | GBA-001 | ADR-004, `toolchain.lock.toml`, SPEC-004/010 em Review, KNO-001/003; hashes upstream registrados |
| GBA-010 | P0 | Scaffold, ABI, GBA IR mínima e LLVM objeto/link | IN_PROGRESS | GBA-005 | Fixture O0/O2 emitida como Mach-O, linkada e executada em macOS arm64; validação Linux x86-64 ainda pendente |
| GBA-020 | P0 | Intake, decoder e primeiro homebrew headless | IN_PROGRESS | GBA-010 (gate Linux x86-64 ainda pendente) | SPEC-012/023/024; Tiny Counter e fixtures headless passaram o gate estritamente AOT em O0/O2 em macOS arm64; SWI #1 e LSL Thumb imediato verificados; LeafGreen estática agora para em `0x080003B4`, raw `0x4A2C` (LDR literal inferido), sem pacote/execução |
| GBA-025 | P0 | CFG, ARM/Thumb, dispatcher, guards e overlays | IN_PROGRESS | GBA-020 (LeafGreen ainda falha no decoder) | SPEC-003/013–024 implementadas em subset; BX, PUSH, MOV high, MOVS, LSL imediato, BL Thumb e SWI #1 passam fixtures O0/O2, incluindo LSL→BX com alvo estático derivado; LeafGreen passou estaticamente pelo retorno de SWI #1, LSL em `0x080003B2` e parou em `0x080003B4` (`0x4A2C`, LDR literal inferido); outras instruções, overlays e runtime geral seguem ausentes |
| GBA-030 | P0 | Bus, timing, DMA, timers e IRQ | IN_PROGRESS | GBA-020 | SPEC-005/016; apenas STR word em EWRAM/IWRAM física passa fixture; mirrors, MMIO, timing/eventos, DMA/timers/IRQ ainda ausentes |
| GBA-040 | P0 | PPU software e APU canônica | BACKLOG | GBA-030 | SPEC-006; gráfico e áudio homebrew |
| GBA-045 | P0 | HLE mínimo, backups e estado | IN_PROGRESS | GBA-030 (HLE RegisterRamReset v1 tem estado ABI v4 próprio; backup/save-state seguem dependentes do bus) | SPEC-007/023; somente RegisterRamReset #1 com latches parciais e memória definida; backup, save/restore e demais SWIs pendentes |
| GBA-050 | P0 | Driver AOT, passes O0/O2 e cache | BACKLOG | GBA-025, GBA-030 | SPEC-004; mesma execução em ambos os pipelines |
| GBA-060 | P0 | Parser/perfis, hints e digests | BACKLOG | GBA-025, GBA-050 | SPEC-008; exemplos e rejeições |
| GBA-070 | P0 | Frontend/menu, input e configurações | BACKLOG | GBA-040, GBA-045, GBA-060 | SPEC-009; fluxo sem mouse e hotplug |
| GBA-080 | P0 | Pacotes Linux x86-64/macOS arm64 | BACKLOG | GBA-070 | SPEC-010/011; máquina sem LLVM |
| GBA-090 | P1 | Qualificar cenário LeafGreen BPGE revisão 1 | BACKLOG | GBA-080 | KNO-004, perfil draft; boot/input/save e cobertura medida |
| GBA-095 | P1 | Windows x86-64/Linux arm64 | BACKLOG | GBA-080 | SPEC-010; adaptadores e matriz própria |
| GBA-100 | P2 | Mobile/Web, link e cartuchos especiais | BACKLOG | GBA-090 | Specs incrementais e corpus selecionado |

## Gates

G0: contrato revisado e toolchains fixadas. G1: objeto nativo executa ALU guest. G2: homebrew produz vídeo/áudio/input/save. G3: pacote e menu portáteis. G4: cenário de jogo documentado, com rótulo strict-aot ou aot-compatible correto.

O runtime de hardware geral ainda não existe. GBA-005 fecha o baseline documental. Em GBA-010, a fixture O0/O2 passou em macOS arm64 nativo; Linux x86-64 segue pendente porque o Docker local é arm64 e a imagem x86-64 exigiu autenticação do registry. Em GBA-020/025/030, Tiny Counter, ABI v2/v3/v4, MSR, literal ROM, ADD PC, STR para EWRAM/IWRAM, MOV ARM, BX/interworking, PUSH Thumb, MOV high, MOVS, LSL imediato, BL/retorno e SWI #1 RegisterRamReset passaram nas fixtures headless; O0/O2 foram cobertos nos casos aplicáveis, incluindo rejeições sem pacote parcial. Ainda não há modelo geral de bus nem hardware GBA. A comparação estática em KNO-005 mostra que erro de compile localiza lacuna, mas não define sozinho a cobertura; decode, semântica runtime e lowering AOT são rastreados separadamente. Após a SPEC-024, LeafGreen avançou estaticamente pelo LSL em `0x080003B2` e parou em `0x080003B4` (`0x4A2C`, LDR literal inferido). Nenhum pacote LeafGreen foi criado, a ROM não foi executada, e REA não gerou decompilação.

## Verificação registrada

Em 2026-10-13: `cargo test --locked --workspace` passou com 27 testes em `gba-ir` e 30 em `gba-recomp`; `scripts/verify-gba020.sh` passou integralmente em Darwin/arm64 em 137,68 s, incluindo sete fixtures LSL O0/O2, propagação LSL→BX e três encodings de shift rejeitados sem pacote parcial. A configuração atual usa 10 jobs de Cargo e 10 threads de LLD no host de 10 CPUs lógicas; amostras pontuais durante links chegaram a aproximadamente 831% de CPU, sem benchmark de throughput sustentado nem comparação de tempo antes/depois. Link e execução continuam verificados apenas em macOS arm64; Linux x86-64 ainda não foi executado.

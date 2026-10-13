# Roadmap

Fonte única de andamento. Data: 2026-10-13. `DONE` exige evidência; `BACKLOG` ainda não começou; `IN_PROGRESS` indica trabalho iniciado, com gates pendentes. Estados Draft/Review das specs indicam revisão documental; aceite de ADR e seleção de toolchain não significam implementação executável.

| ID | Prioridade | Entrega | Estado | Dependências | Contrato/evidência |
|---|---|---|---|---|---|
| GBA-001 | P0 | Especificação SDD inicial, referência e inventário | DONE | — | README, 16 specs, ADR-001–003, KNO-001/002/004/005, ambos vendors fixados por commit; revisão documental local |
| GBA-005 | P0 | Revisar contratos e fechar versões/toolchains | DONE | GBA-001 | ADR-004, `toolchain.lock.toml`, SPEC-004/010 em Review, KNO-001/003; hashes upstream registrados |
| GBA-010 | P0 | Scaffold, ABI, GBA IR mínima e LLVM objeto/link | IN_PROGRESS | GBA-005 | Fixture O0/O2 emitida como Mach-O, linkada e executada em macOS arm64; validação Linux x86-64 ainda pendente |
| GBA-020 | P0 | Intake, decoder e primeiro homebrew headless | IN_PROGRESS | GBA-010 (gate Linux x86-64 ainda pendente) | SPEC-012; Tiny Counter, 6 instruções estritamente AOT, O0/O2 executados em macOS arm64; LeafGreen bloqueada por opcode ainda não suportado |
| GBA-025 | P0 | CFG, ARM/Thumb, dispatcher, guards e overlays | IN_PROGRESS | GBA-020 (LeafGreen ainda falha no decoder) | SPEC-003/013/014/015/016/017; ABI v2/v3, MSR CPSR bancado, literal ROM, ADD PC, STR RAM física e MOV registrador sem shift verificados O0/O2; cobertura por família em KNO-005; branch exchange/Thumb e runtime geral ausentes |
| GBA-030 | P0 | Bus, timing, DMA, timers e IRQ | IN_PROGRESS | GBA-020 | SPEC-005/016; apenas STR word em EWRAM/IWRAM física passa fixture; mirrors, MMIO, timing/eventos, DMA/timers/IRQ ainda ausentes |
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

O runtime de hardware geral ainda não existe. GBA-005 fecha o baseline documental. Em GBA-010, a fixture O0/O2 passou novamente em macOS arm64 nativo; Linux x86-64 segue pendente porque o Docker local é arm64 e a imagem x86-64 exigiu autenticação do registry. Em GBA-020/025/030, Tiny Counter, ABI v2/v3, MSR, literal ROM, ADD PC, STR para EWRAM/IWRAM e MOV sem shift passaram nas fixtures headless; O0/O2 foram cobertos nos casos aplicáveis, incluindo predicado falso, regiões não suportadas, desalinhamento e rejeição de literal fora da ROM. Ainda não há modelo geral de bus nem hardware GBA. A comparação estática em KNO-005 mostra que erro de compile localiza lacuna, mas não define sozinho a cobertura; decode, semântica runtime e lowering AOT são rastreados separadamente. Após SPEC-017, a descoberta LeafGreen avançou até `0x08000230` (`0xE12FFF11`) e foi recusada; `BX r1` é inferência pelos campos ARM, sem pacote ou execução do jogo. KNO-004 preserva hash e sequência de limites; REA recusou o formato raw e não gerou decompilação.

## Verificação registrada

Em 2026-10-13: a verificação documental prévia validou 94 links Markdown relativos, 5 arquivos TOML e 3 JSON. Para SPEC-017: `cargo fmt --all -- --check`, `cargo test --locked -p gba-ir -p gba-recomp` (22 testes), `sh -n scripts/verify-gba020.sh scripts/verify-gba010.sh`, `python3 -B -m py_compile fixtures/tiny-counter/make_rom.py`, links Markdown locais, `git diff --check` e `scripts/verify-gba020.sh` passaram. O script executou as fixtures O0/O2, incluindo MOV PC e condição falsa, C harnesses ABI v2/v3, stores RAM e faults esperados sem pacote parcial. A compilação LeafGreen falhou em `0x08000230` antes de publicar pacote; o jogo não foi executado. As verificações anteriores `scripts/verify-gba010.sh` e o link C de ABI v1 continuam registradas. Link e execução são macOS arm64; Linux x86-64 não foi executado. Os builds mantiveram Cargo em um job e LLD em até dois threads.

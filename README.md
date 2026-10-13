# GBA Recomp — especificação

Projeto de recompilador estático **GBA → GBA IR → LLVM IR → plataforma host**, com runtime de hardware, perfis de ajuste fino por jogo e menu de vídeo, áudio e controles.

```text
ROM .gba + perfil por SHA-256 + símbolos opcionais
  → intake → descoberta ARMv4T/Thumb → GBA IR verificada
  → LLVM IR → objeto para target triple → link com runtime e frontend
                                          ↓
                       executável nativo + assets + manifesto
```

O LLVM traduz a CPU; o runtime reproduz barramento, PPU, APU, DMA, timers, IRQ, BIOS e cartucho. “Qualquer plataforma” é uma arquitetura extensível: cada destino precisa de backend LLVM, toolchain de link, runtime e adaptadores host. Não significa suporte automático a todo SO.

O bootstrap implementa um slice ALU de GBA IR para LLVM e ABI C; a fixture O0/O2 já emite, linka e executa em macOS arm64. A validação Linux x86-64 segue pendente. Ainda não há intake, recompilação de ROM ou jogo executável. O método SDD segue `../switch-recomp-research`, adaptado ao GBA. [ROADMAP.md](ROADMAP.md) é a fonte de progresso.

- [Arquitetura](docs/architecture.md) e [fluxo do usuário](docs/product-flow.md).
- [Especificações](specs/README.md), [decisões](adr/README.md) e [plano](plans/PLAN-001-delivery.md).
- [Baseline de toolchain](toolchain.lock.toml) e [decisão ADR-004](adr/ADR-004-toolchain-baseline.md).
- [Scaffold Rust](Cargo.toml) e [verificação da fixture GBA-010](scripts/verify-gba010.sh).
- [Evidências](knowledge/KNO-001-evidence.md), [uso do REA](knowledge/KNO-002-rea.md) e [questões abertas](knowledge/KNO-003-open-questions.md).

## Validar o slice GBA-010

Instale as versões e dependências do host descritas em [toolchain.lock.toml](toolchain.lock.toml), defina `LLVM_SYS_231_PREFIX` para a raiz extraída de LLVM 23.1.3 e execute:

```sh
scripts/verify-gba010.sh
```

O script emite O0/O2, verifica o formato do objeto, linka e executa o harness C e grava o relatório sob `target/gba010/`. A validação nativa atual é macOS arm64; Linux x86-64 permanece pendente no roadmap.
- [Perfil por jogo](specs/SPEC-008-game-profiles.md) e [menu de configuração](specs/SPEC-009-settings-menu.md).
- [Exemplos TOML](profiles/README.md) para contratos ainda não implementados.

Referências locais: `vendor-ref/gba-recomp` e `vendor-ref/mgba`. O mGBA foi clonado de https://github.com/mgba-emu/mgba.git; commits e licenças estão no registro de evidências. `vendor-ref/` permanece ignorado pelo Git. Uma ROM local foi identificada em [KNO-004](knowledge/KNO-004-rom-intake.md), com [perfil draft](profiles/leafgreen-bpge-rev1.toml). Compatibilidade comercial ainda não foi demonstrada. `roms/` também está ignorado pelo Git.

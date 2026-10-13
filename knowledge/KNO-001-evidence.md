# KNO-001 — Referências e observações

Data: 2026-10-13. Escopo: leitura de fontes locais e documentação LLVM; sem execução de ROM.

## Identidade

| Fonte | Revisão observada | Papel |
|---|---|---|
| `../switch-recomp-research` | `ad679f2f177f757d42a7d4d35915fd4dbe304daf` | Técnica SDD, templates, separação IR/runtime/host |
| `vendor-ref/gba-recomp` — JRickey/gba-recomp | `de4edf59b872d887046d6a3b005e2df551b6d44c` | Recompilação GBA e UX |
| `vendor-ref/mgba` — mgba-emu/mgba | `143adf5aa2a60c6036810b0cd72445e13d74b0e6` | Escopo de hardware e futuro oráculo |

mGBA clonado nesta entrega por `git clone https://github.com/mgba-emu/mgba.git vendor-ref/mgba`. Ambos os vendors apresentavam `git status --short` vazio após o clone. As instruções do projeto Switch foram estudadas como referência de método, sem copiar contratos Horizon/AArch64 para ARMv4T.

## Registro de observações

`confirmed` significa confirmado na fonte indicada, não validado em silício.

| ID | Confiança | Observação e fonte local | Consequência |
|---|---|---|---|
| SRC-01 | confirmed | Switch `AGENTS.md`, `specs/_template.md`, `adr/_template.md`: KNO→ADR→plano→spec→implementação→verificação | Mesma organização documental |
| SRC-02 | confirmed | gba-recomp `crates/recomp-core/src/build.rs`: C e compilador externo/TinyCC; opções de target/Fast/Ship ainda têm TODO | LLVM direto é nova arquitetura |
| SRC-03 | confirmed | gba-recomp `crates/recomp-core/src/emit.rs`: guard de RAM e `interp_one`; `docs/labels.md`: labels por hash e snapshots IWRAM | Não afirmar AOT integral do vendor |
| SRC-04 | confirmed | mGBA `include/mgba/internal/gba/memory.h:26`: regiões; `:60`: BIOS 16 KiB, EWRAM 256 KiB, IWRAM 32 KiB, VRAM 96 KiB | Bus explícito, mirrors e efeitos |
| SRC-05 | confirmed | mGBA `include/mgba/internal/gba/gba.h:22`: 16.777.216 Hz; `video.h:29`: 1232 ciclos/linha, 228 linhas, 280896 ciclos/frame | Taxa derivada ≈59,7275 Hz, não 60 fixos |
| SRC-06 | confirmed | mGBA `src/gba/memory.c:476`, tabelas waitstates/prefetch; `src/gba/dma.c:259`: DMA bloqueia CPU e computa custos | Ciclos constantes por bloco são insuficientes em geral |
| SRC-07 | confirmed | mGBA `src/gba/gba.c:327`: processamento de eventos; `timer.c`, `audio.c`, `video.c` | Scheduler central com checkpoints |
| SRC-08 | confirmed | mGBA `src/gba/bios.c`, `savedata.c`, `overrides.c`, `sio.c`, `include/mgba/internal/gba/cart/` | BIOS, backups, RTC, sensores e serial têm contratos próprios |
| SRC-09 | confirmed | mGBA `include/mgba/internal/arm/arm.h:29`: modos CPU, bancos e vetores | Modelar estado arquitetural completo, sem omitir modos por suposição |
| SRC-10 | confirmed | gba-recomp `docs/architecture.md`, `docs/launcher.md`, `crates/input-config/`, `crates/screen/` | Referência de UX e apresentação; afirmações de compatibilidade do texto não foram reproduzidas |

## Detalhamento da referência GBA

- `crates/recomp-core/src/analyze.rs:24`: blocos têm endereço e modo; `:136` usa entradas observadas de RAM.
- `crates/gba-core/src/capi.rs:19`: estado C ABI e callbacks; `:36` contém gates de execução/eventos.
- `crates/gamedb/src/lib.rs:39`: indexação por SHA-256.
- `crates/input-config/src/lib.rs:230`: teclado/gamepad; `:400`: opções A/V; `:508`: persistência global.
- `crates/menu/src/lib.rs:78`: menu em jogo; remapeamento fica no launcher (`crates/launcher/src/screens/input.rs:40`).

Inferência de projeto: o novo menu precisa integrar remapeamento e as opções de dispositivo/volume/latência, com herança por jogo; a existência do menu vendor não demonstra esses requisitos completos.

## Fontes LLVM

O [LangRef](https://llvm.org/docs/LangRef.html) e o [manual de UB](https://llvm.org/docs/UndefinedBehavior.html), consultados em 2026-10-13, documentam poison, overflow e shifts. Implicação de projeto: o lowering deve implementar explicitamente os casos ARM definidos que não coincidem com operações LLVM ingênuas.

[Cross-compilation LLVM](https://llvm.org/docs/HowToCrossCompileLLVM.html), consultada na mesma data, exige configuração de destino e sysroot. A proposta distingue emissão, link e suporte host.

## Baseline de toolchain — GBA-005

Pesquisa de fontes oficiais em 2026-10-13; os hashes abaixo identificam entradas fixadas, não evidência de build:

| ID | Observação de fonte | Evidência fixada / limite |
|---|---|---|
| TOOL-01 | O anúncio oficial publicou Rust stable 1.99.0 em 2026-10-01; as release notes registram que o LLVM interno do `rustc` foi atualizado para LLVM 23. [Anúncio](https://blog.rust-lang.org/2026/10/01/Rust-1.99.0/), [release notes](https://doc.rust-lang.org/stable/releases.html) | Manifesto `channel-rust-1.99.0.toml`, SHA-256 `ce6dddc886364f8d786514771212cebe9b731ba82d6b859951c6b0ccc516b6a2`. A seleção de stable 1.99.0 é decisão em ADR-004; o LLVM interno do `rustc` não é o LLVM usado pelo backend AOT. |
| TOOL-02 | A página oficial LLVM anuncia 23.1.3 em 2026-10-06 e lista pacotes Linux x86-64 e macOS Apple Silicon. [Anúncio LLVM](https://discourse.llvm.org/t/llvm-23-1-3-released/92432), [release oficial](https://github.com/llvm/llvm-project/releases/tag/llvmorg-23.1.3) | Tag `llvmorg-23.1.3` aponta para commit `0d261d1ca552c95a8f007e061c787ac7132fbcbc`. SHA-256 dos pacotes Linux e macOS e do source tarball estão em [toolchain.lock.toml](../toolchain.lock.toml). Isso não prova que o ambiente instalou ou usou os pacotes. |
| TOOL-03 | O pacote publicado da crate `llvm-sys` fornece bindings da API C e declara `links = "llvm-23"`, versão 231.0.1. [Versão 231.0.1](https://crates.io/crates/llvm-sys/231.0.1), [Cargo.toml dessa distribuição](https://docs.rs/crate/llvm-sys/231.0.1/source/Cargo.toml.orig) | Pacote `.crate` 231.0.1, SHA-256 `21cdb31b2047b57121a31a911af86e4bacb93d165843d27cea1e14e071ac2b4c`. A crate é passivamente mantida; pin e wrapper local restringem o impacto, mas não substituem revisão de upgrade. |
| TOOL-04 | A API C LLVM tem garantias de estabilidade limitadas. O bootstrap desliga o link automático de `llvm-sys` para selecionar componentes explicitamente. [README da versão 231.0.1](https://docs.rs/crate/llvm-sys/231.0.1/source/README.md) | O `build.rs` de `gba-llvm` consulta `llvm-config --version` e falha exceto em 23.1.3. A checagem própria é exata; não depende da feature de versionamento da crate. |
| TOOL-05 | O pacote oficial macOS arm64 `LLVM-23.1.3-macOS-ARM64.tar.xz` foi baixado e extraído em `/tmp`; SHA-256 calculado (`3dcfd88aec10be204a24037bcb0dc0ad777882e9dbe8ac60e2639060d8a794b1`) confere com o lock. Seu `llvm-config --shared-mode` reporta `static`; `--libnames --link-shared` falha por ausência de `libLLVM-23.dylib`. | Evidência local de conteúdo e configuração do pacote fixado. A decisão é linkar apenas os componentes estáticos declarados, sem deixar `llvm-sys` selecionar a biblioteca completa. Emissão, link e execução da fixture ainda não foram comprovados. |
| TOOL-06 | O pacote inclui `lld`/`ld64.lld` 23.1.3 e arquivos estáticos com bitcode LLVM 23. Apple `ld` 17 rejeita esses atributos de bitcode; `clang` LLVM 23 com `lld` e SDK macOS 15.5 linkou um probe C arm64. Um link exploratório de todo o LLVM via LLD leu o bitcode, mas falhou em símbolos de runtime C++ e incluiu bibliotecas de mais. | Evidência dos erros reais e do probe na máquina atual. Os archives LLVM avisam que foram compilados com deployment target macOS 14.0. A correção usa só componentes selecionados e runtime estático, validada em TOOL-08. |
| TOOL-07 | Um link do binário Rust com runtimes C++ dinâmicos do pacote produziu Mach-O, mas a execução com `DYLD_LIBRARY_PATH` apontando ao pacote terminou com `Bus error: 10`; dyld registrou símbolos C++ ausentes ao substituir `libc++abi` do sistema. | Evidência de que o override global e o runtime C++ dinâmico LLVM não são seguros neste host. Corrigido ao linkar libc++, libc++abi e libunwind estaticamente; resultado validado em TOOL-08. |
| TOOL-08 | `scripts/verify-gba010.sh` passou em macOS arm64 nativo com Rust 1.99.0 e o pacote LLVM arm64 cujo hash consta em TOOL-05: emitiu O0/O2 Mach-O; LLVM confirmou o DataLayout e tamanhos/offsets ABI; o Apple linker linkou o harness C; os dois executáveis passaram soma modular, avanço PC/ciclo, CPSR inalterado, overflow de ciclo e ABI incompatível sem mutação do estado guest. | Relatório `target/gba010/macos-arm64/toolchain-report.txt`: LLD 23.1.3, sysroot Xcode 15.5, Apple Clang/ld para o harness. O linker avisou que o objeto relocável não inclui load command de plataforma e assumiu macOS; link e execução passaram. O executável fixture foi inspecionado para garantir ausência de `libLLVM`/`libLTO` dinâmicas. Isso valida o slice e o host macOS, não uma ROM nem o pacote de produto. |
| TOOL-09 | O pacote oficial LLVM Linux x86-64 foi baixado, teve SHA-256 comparado ao lock e extraído. Tentativa de executar `ubuntu:24.04` em Docker `linux/amd64` falhou porque o registry exigiu autenticação; o daemon local informa `linux/arm64`. | Emissão/link/execução Linux x86-64 continuam sem prova. Não inferir suporte nativo de pacote ou toolchain cross. |

### Observação do ambiente de trabalho

Inventário inicial em 2026-10-13: `arm64`, macOS 15.3.1, Xcode 16.4 (build 16F6), Apple Clang 17.0.0 (1700.0.13.5), CMake 4.4.3 e `rustup` presentes. `rustc`, `cargo`, `llvm-config` e Ninja não estavam no PATH. Uma tentativa de consultar stable sincronizou metadados/componentes, mas não deixou compilador global executável. Rust 1.99.0 foi instalado isoladamente em `/tmp/gba-recomp-rustup`; `cargo check --workspace --locked` passou. A saída `rustc --version --verbose` informa LLVM interno 23.1.1, separado do LLVM AOT 23.1.3. A fixture GBA-010 passou O0/O2 em macOS arm64 segundo TOOL-08. O pacote Linux x86-64 também teve o hash validado, mas não foi executado: Docker local é Linux arm64 e a imagem amd64 exigiu autenticação no registry.

## Licenças observadas

mGBA: `LICENSE` declara MPL-2.0; terceiros têm seus próprios avisos. gba-recomp: Cargo declara MIT OR Apache-2.0; base gamedb tem CC0; `THIRD-PARTY.md` registra TinyCC LGPL-2.1. Não assumir que tudo no vendor tem uma licença única. Nenhum código vendor foi transplantado nesta entrega. Uma futura reutilização precisa registrar arquivos, revisão e avisos correspondentes.

## Limites

Sem benchmarking, decompilação ou execução de jogos. Afirmações do vendor como compatibilidade ampla e custo desprezível de fallback permanecem não verificadas. Constantes de referência não substituem testes de borda de HBlank, bus e IRQ. A seleção de Rust/LLVM e o slice ALU estão exercitados apenas em macOS arm64; Linux x86-64, SDL e garantias por jogo continuam sem prova.

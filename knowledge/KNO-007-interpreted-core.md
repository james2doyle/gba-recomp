# KNO-007 — Evidência do núcleo interpretado `gba-core`

Data: 2026-10-14. Host: Darwin arm64 (Mac de 10 CPUs lógicas), Rust 1.99.0, build `--release`.

## Observação de execução

| ID | Cenário | Resultado |
|---|---|---|
| EV-007-01 | LeafGreen BPGE rev.1, SHA-256 `2f978f635b9593f6ca26ec42481c53a6b39f6cddd894ad5c062c1419fac58825`, HLE BIOS v1, save inicial vazio (Flash 128 KiB detectada), 600 frames sem input | Sem fault; ~59,8 M instruções interpretadas; ~690 frames/s no host; frame 600 mostra a tela "GAME FREAK presents" |
| EV-007-02 | Mesma ROM, 3000 frames, START nos frames 1200/1500 e A a cada 200 frames de 1700 a 2700 | Sem fault; 299.625.419 instruções interpretadas; 842.604.233 ciclos guest; hash final `72e7569f9537f038`. Capturas manuais mostraram a intro de batalha (frame 1200), o tutorial de controles de novo jogo (1800) e o início do diálogo do Prof. Oak (3000). Relatório JSON: [evidence/gba055-core-leafgreen-3000f-20261014.json](evidence/gba055-core-leafgreen-3000f-20261014.json) |

SWIs observados: 0x01, 0x0B, 0x0C, 0x0F, 0x11, 0x12. MMIO não modelado acessado: registradores de DMA/timer não usados na inicialização em massa (0x0AA–0x11E), SIOCNT (0x128) e 0x15C/0x15E.

## Interpretação e limites

- Isto é execução **interpretada** (`mode: interpreted`), não recompilação. Não conta como strict-AOT nem como `aot-compatible` (SPEC-062).
- A comparação visual foi feita por inspeção humana de capturas locais; as capturas não foram versionadas (conteúdo proprietário). O diferencial contra mGBA está na seção seguinte.
- O save do repositório está vazio (0xFF), então o caminho "Continue" não foi exercitado.
- Timing de prefetch e de acesso é aproximado (SPEC-057 BUS-57-04); hashes não devem ser comparados com outros emuladores sem diferencial.

## Diferencial contra mGBA (VAL-03)

Oráculo externo: mGBA `0.11.0-dev` (`projectVersion` `0.11-9155-143adf5aa`), commit `143adf5aa2a60c6036810b0cd72445e13d74b0e6` de `vendor-ref/mgba`, licença MPL-2.0. A libmgba é compilada fora da árvore (`target/mgba-build`) e ligada apenas à ferramenta de teste `tools/mgba-oracle`; nenhum código do mGBA foi copiado para crates do produto e `vendor-ref/` não foi alterado. Build: `COLOR_16_BIT` (saída BGR555 e blending em 5 bits), sem core GB, frontends, debuggers ou scripting; execução com HLE BIOS (`useBios=0`, `skipBios=1`) e `idleOptimization=ignore`. Os dois lados expandem BGR555 com `(v<<3)|(v>>2)` e calculam o mesmo hash FNV-1a por frame.

Reprodução: `scripts/build-mgba-oracle.sh` e depois `scripts/diff-mgba.sh --rom <rom> --frames 3000 --every 100 --window 10 --input ...` (cargo no `PATH`). Relatório numérico: [evidence/gba055-mgba-diff-leafgreen-20261014.json](evidence/gba055-mgba-diff-leafgreen-20261014.json).

### Observação de execução

| ID | Cenário | Resultado |
|---|---|---|
| EV-007-03 | Cenário do EV-007-02 (3000 frames, mesmos inputs), gba-core `8618314` vs mGBA `143adf5a` | 1539/3000 frames com hash idêntico no mesmo índice; primeiro frame divergente: 16. Amostras a cada 100 frames: 15/30 idênticas no mesmo frame, 26/30 idênticas a algum frame do mGBA em ±10. Nos 1216 frames em que nossa imagem muda, o deslocamento até o frame igual do mGBA foi −1 (322), 0 (107), +1 (94), +2 (264), +3 (123), +4 (247), −6 (1); 58 sem par em ±30 |
| EV-007-04 | Frames 2900 e 3000 (cena do Prof. Oak) | 2340 pixels (6,09%) divergentes na caixa 93,23–147,108: o Prof. Oak aparece no mGBA e está ausente no gba-core. Dump de estado no frame 2900: DISPCNT 0x1741 (modo 1, BG2 afim ativo) e BG2CNT 0x5C81 iguais; paleta e OAM idênticas; BG2PA/BG2PD = 0x0000 no gba-core e 0x0100 no mGBA |
| EV-007-05 | Experimento fora da árvore: gravar BG2PA/BG2PD/BG3PA/BG3PD = 0x0100 após o frame 1 (depois do SWI 0x01 do jogo) sem alterar o gba-core | Frame 2900 passa a ter 0 pixels divergentes; hashes iguais sobem de 1539 para 1668/3000; frames sem par de 58 para 39 |
| EV-007-06 | Experimento: iniciar o gba-core com VCOUNT = 0x7E (como `GBASkipBIOS` do mGBA) | Nenhum dos 3000 hashes muda nesse cenário |
| EV-007-07 | Frames restantes sem par (1200, 1593–1609, 1635, 1703, 2305, 2502, ...) | Inspeção visual: estado intermediário de cena que o mGBA não exibe (1200), folhas da tela-título em posições diferentes (1600; OAM com X deslocado ~6 px), fase da seta "próximo" e da animação do Pikachu na caixa de texto (17xx–25xx). Nenhuma diferença de cor, camada, prioridade ou blending fora dessas regiões |

### Inferência

- **Bug provável (alta confiança):** o HLE de `RegisterRamReset` (SWI 0x01, flag 0x80, `crates/gba-core/src/bios.rs`) zera 0x000–0x05F, inclusive BG2PA/BG2PD/BG3PA/BG3PD, e não restaura 0x0100 como fazem a BIOS real e o HLE do mGBA (`src/gba/bios.c`). Pelo EV-007-04/05, a LeafGreen depende do valor deixado pelo SWI (no mGBA eles seguem 0x0100 no frame 2900), então todo BG afim com escala identidade implícita (Prof. Oak em BG2) é amostrado com PA=PD=0 e some. Isso corrige a leitura do EV-007-02: no frame 3000 o diálogo aparece, mas o Prof. Oak não.
- **Timing (média confiança):** os deslocamentos de +1 a +4 (gba-core mostra antes uma imagem que o mGBA mostra depois) concentram-se na intro (≈700–1100) e após inputs, e se dissipam em transições de cena. Isso é coerente com o mGBA ter frames de lag (frame do jogo que ultrapassa um VBlank) que o gba-core não tem, ou seja, o gba-core cobra menos ciclos por instrução (prefetch e waitstates, SPEC-057 BUS-57-04). Os deslocamentos de −1 no início (16–700) indicam o oposto em trechos de boot. A causa ainda não foi isolada (custo de ciclos dos SWIs HLE é candidato); o EV-007-06 descarta a fase inicial de VCOUNT.
- Não há evidência de bug de composição de PPU (cores, prioridade, blending, janelas) nas amostras: toda divergência restante se explica por deslocamento de frames ou estado de animação.

### Limites

- Um único ROM e cenário; o mGBA é segunda referência, não verdade absoluta (VAL-03). Divergência entre oráculos continua questão aberta até confirmação em hardware ou suite independente.
- Capturas e dumps de estado foram inspecionados localmente e não foram versionados (conteúdo derivado da ROM).

## Bugs corrigidos pelos testes unitários

Encontrados ao cobrir as matrizes de aceite de SPEC-057–060 (`cargo test -p gba-core`: 128 testes, 2026-10-14). Após as correções o cenário EV-007-02 foi repetido: sem fault, as mesmas 299.625.419 instruções, 842.604.233 ciclos e hash final `72e7569f9537f038` (nenhum caminho corrigido muda a LeafGreen até o frame 3000).

- `bus.rs` `Timer::default`: `overflow_at` iniciava em 0, então todo timer nunca ligado disparava um overflow espúrio no primeiro evento (pop/underrun de FIFO, pedido de DMA de som, tick de cascade). Agora é `u64::MAX`.
- `bus.rs` `io_write8`/`io_latched16`: writes de byte em IE, WAITCNT, IME e registradores de DMA e de timer eram mesclados com o array `io`, que não guarda esses valores, e zeravam o outro byte do registrador.
- `bus.rs` `timer_write`: o bit de cascade em TM0CNT_H (inexistente no timer 0) congelava a leitura do contador.
- `bus.rs` `dma_count`: count 0x4000 escrito explicitamente nos canais 0–2 virava 1 unidade em vez de 0x4000.
- `bus.rs` `run_dma`: a inferência de tamanho da EEPROM (`eeprom_dma_hint`) nunca era chamada; EEPROM de 512 B ficava com endereçamento de 14 bits.
- `bus.rs` `read_raw32`/`write_raw32`: acessos de 32 bits não alinhavam o endereço (BUS-57-03). Só era alcançável por chamadas internas, porque a CPU já alinha.
- `bus.rs` `Bus::new`: BG2PA/PD e BG3PA/PD iniciavam em 0 em vez de 0x0100 (valor de reset do hardware); modos bitmap/affine sem configuração explícita mostravam uma única coluna.
- `apu.rs` SOUNDCNT_X: desligar o master zerava também SOUNDCNT_H (0x82), que deve continuar legível e gravável.
- `apu.rs` `clock_sweep`: sweep de diminuição com shift 0 levava a frequência a 0 em vez de mantê-la.

## Execução recompilada (`aot-compatible`, SPEC-062/063)

| ID | Cenário | Resultado |
|---|---|---|
| EV-007-08 | LeafGreen, `compile --opt O2 --mode aot-compatible --profile-frames 3000` com os inputs do EV-007-02; execução `--headless --frames 3000 --verify-interpreter` com os mesmos inputs | Compilação 54 s (emissão 48 s em 10 threads, RSS 1,28 GB): 2845 runs, 28.719 instruções nativas, 7 sítios de shell. Execução sem divergência contra o interpretador em ciclos, registradores e framebuffer; 248.506.141 instruções nativas e 51.119.278 interpretadas (82,9% nativo; o restante é código em RAM, shells e misses); hash final `9dbc61778121f97e`, igual ao do mGBA após a correção do reset afim; ~980 frames/s sem verificação |
| EV-007-09 | Mesmo pacote em janela com `SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy`, 600 frames | Saída 0, áudio ligado, SDL3 carregada do pacote via `@rpath`. Janela real, menu nativo, som e gamepad não foram verificados nesta sessão |
| EV-007-10 | Código em RAM nativo (SPEC-065) e `bundle` (SPEC-066): pacote O0 com perfil de 600 frames; bundle O2 com perfil de 3000 frames e os inputs do EV-007-02 | O0/600: 79,4% → 99,92% nativo (21 runs de RAM, 359 instruções, 2.516 comparações de guard, 0 falhas), sem divergência. Bundle: 2881 runs (36 de RAM), executável 24,6 MB com a ROM; `--headless --frames 3000 --verify-interpreter` sem `--rom`: 99,92% nativo, sem divergência, hash `9dbc61778121f97e`; ~914 frames/s |

## Questões abertas

- Isolar a origem do lag diferente (custos de prefetch/waitstate vs. custo de SWIs HLE) com trace de ciclos por frame.
- Repetir o diferencial após corrigir o reset afim do SWI 0x01.
- Gravação e releitura de save Flash na LeafGreen.
- Underruns de FIFO também são contados para um FIFO sem roteamento L/R (o pop ocorre sempre que o timer selecionado transborda). Falta confirmar contra hardware/mGBA se a métrica deve ignorar FIFOs desabilitados.

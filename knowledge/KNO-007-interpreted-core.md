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
- A comparação visual foi feita por inspeção humana de capturas locais; as capturas não foram versionadas (conteúdo proprietário). O diferencial contra mGBA ainda não existe.
- O save do repositório está vazio (0xFF), então o caminho "Continue" não foi exercitado.
- Timing de prefetch e de acesso é aproximado (SPEC-057 BUS-57-04); hashes não devem ser comparados com outros emuladores sem diferencial.

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

## Questões abertas

- Diferencial de framebuffer/ciclos contra mGBA nos mesmos frames.
- Gravação e releitura de save Flash na LeafGreen.
- Underruns de FIFO também são contados para um FIFO sem roteamento L/R (o pop ocorre sempre que o timer selecionado transborda). Falta confirmar contra hardware/mGBA se a métrica deve ignorar FIFOs desabilitados.

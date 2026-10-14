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

## Questões abertas

- Diferencial de framebuffer/ciclos contra mGBA nos mesmos frames.
- Gravação e releitura de save Flash na LeafGreen.

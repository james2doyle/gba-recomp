# SPEC-066 — `bundle`: executável recompilado com a ROM embutida

Estado do documento: Accepted em 2026-10-14. Implementada e verificada em macOS arm64: `bundle` da LeafGreen (O2, perfil 3000 frames, 54 s) gera `game` de 24,6 MB que roda sem `--rom`; 3000 frames headless verificados contra o interpretador (hash `9dbc61778121f97e`), ~914 fps; janela com driver dummy por 300 frames. Progresso: [ROADMAP](../ROADMAP.md).

## Problema e escopo

Pacotes `aot-compatible` ([SPEC-062](SPEC-062-native-blocks-aot-compatible.md)) exigem `--rom` em toda execução, por decisão de não distribuir conteúdo do jogo. Para uso pessoal, o usuário pediu um comando que gere o binário já com a ROM. Esta spec adiciona essa opção explícita sem mudar o padrão de `compile`.

## Requisitos normativos

- BND-66-01: `gba-recomp bundle --rom R --out D [opções de compile]` MUST gerar um pacote `aot-compatible` cuja imagem executável contém a ROM como constante (`gba_aot_rom`), com padrões O2 e perfil de 3000 frames; `--mode` é recusado (sempre `aot-compatible`). `compile --embed-rom` produz o mesmo efeito.
- BND-66-02: o executável MUST rodar sem `--rom`; se `--rom` for passado, o arquivo prevalece e continua validado pelo SHA-256 embutido.
- BND-66-03: o manifesto MUST registrar `"rom_embedded": true`, e o comando MUST avisar que o pacote contém o jogo e não deve ser redistribuído. `compile` sem `--embed-rom` continua sem ROM no pacote.
- BND-66-04: a ABI nativa passa a v2: `gba_native_main(argc, argv, entries, count, sha256, guards, guard_count, rom, rom_len)`, com `rom = NULL, rom_len = 0` quando não embutida.

## Critérios de aceite

Bundle da LeafGreen executando sem `--rom` em modo headless com `--verify-interpreter` por 3000 frames sem divergência e em janela (smoke com driver dummy); `compile` normal continua exigindo `--rom`.

## Referências

[SPEC-062](SPEC-062-native-blocks-aot-compatible.md), [SPEC-063](SPEC-063-recompiled-frontend-settings.md), [SPEC-065](SPEC-065-native-ram-code-guards.md).

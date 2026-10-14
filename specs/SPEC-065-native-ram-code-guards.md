# SPEC-065 — Código em RAM nativo com guards de conteúdo

Estado do documento: Accepted em 2026-10-14. Implementada: snapshots de RAM no perfil (até 4 variantes, 4 KiB), runs com guard na tabela, verificação por geração de página de 1 KiB + comparação de bytes, saída em store sobre página com código. Verificado em macOS arm64: teste unitário do guard (bytes alterados → miss, restaurados → nativo, sem escrita → sem comparação); LeafGreen O0 600 frames: 79,4% → 99,92% nativo com 21 runs de RAM, 2.516 comparações de guard e 0 falhas; bundle O2 3000 frames com input: 99,92% nativo, sem divergência contra o interpretador (KNO-007 EV-007-10). Progresso: [ROADMAP](../ROADMAP.md).

## Problema e escopo

Na LeafGreen, ~17% das instruções executadas em pacotes `aot-compatible` ([SPEC-062](SPEC-062-native-blocks-aot-compatible.md)) vêm de rotinas copiadas para IWRAM/EWRAM (mixer de som, handlers), que hoje rodam no interpretador. Esta spec compila essas rotinas e cumpre IAC-08 da [SPEC-055](SPEC-055-interpreter-assisted-compatibility.md): código mutável só executa nativo enquanto seus bytes forem idênticos aos compilados.

## Requisitos normativos

- RAM-65-01 (perfil): a execução de perfil MUST registrar, para cada entrada executada em EWRAM (0x02) e IWRAM (0x03), o modo ISA e um snapshot dos bytes a partir do PC (até o fim do run ou 4 KiB). Se a mesma entrada executar com bytes diferentes, uma nova variante é registrada, até 4 por entrada.
- RAM-65-02 (compilação): cada variante MUST virar um run com guard `{endereço, tamanho, bytes esperados}` referenciado pela tabela de entradas. Runs de ROM não têm guard. O decode usa somente o snapshot; alvos de branch fora dele não são seguidos.
- RAM-65-03 (guard): antes de chamar um run com guard, o runtime MUST verificar que os bytes atuais da RAM coincidem com os esperados; caso contrário tenta a próxima variante e, sem nenhuma válida, interpreta (miss). A verificação não consome ciclos guest. Um contador de gerações de escrita por página de 1 KiB permite pular a comparação quando nenhuma página do run foi escrita desde a última validação.
- RAM-65-04 (invalidação): uma store executada por código nativo que atinja página com código compilado MUST completar a instrução e sair para o dispatcher, que revalida antes da próxima execução nativa. DMA e stores interpretadas incrementam a geração da página.
- RAM-65-05 (equivalência): NB-62-01 continua valendo: `--verify-interpreter` MUST permanecer sem divergência.

## Critérios de aceite

Teste do guard (bytes alterados → miss; restaurados → nativo); diferencial LeafGreen O2 3000 frames sem divergência; proporção nativa e contagem de misses registradas antes/depois.

## Referências

[SPEC-055](SPEC-055-interpreter-assisted-compatibility.md) IAC-08, [SPEC-062](SPEC-062-native-blocks-aot-compatible.md).

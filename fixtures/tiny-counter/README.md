# Tiny Counter

Esta é uma ROM homebrew determinística para o primeiro fluxo ROM→AOT→executável host. O fonte gerador em `make_rom.py` grava um header GBA válido, um branch de entrada e cinco instruções no corpo: `MOV r0,#0`, `ADD r0,r0,#1`, `CMP r0,#10`, `BNE` para o incremento e um branch próprio ao terminar.

O mesmo gerador tem modos de ROM pequenas para validar incrementos isolados da ISA, interworking e ABI. Por exemplo, `--mode thumb-lsl-bx` desloca uma constante em Thumb e usa o resultado num `BX`; isso verifica se a descoberta propaga o valor até o destino ARM. Esses modos são fixtures sintéticas, não ROMs de jogos comerciais.

`--mode thumb-ldr-literal-bx` coloca a instrução em PC com `PC % 4 == 2`, lê um word do pool ROM e fecha o destino de `BX` estaticamente. `thumb-ldr-literal-out-of-rom` confirma a rejeição anterior à publicação de pacote.

O binário é criado durante a verificação em `target/` e não é versionado. O executável host repete funções LLVM nativas por até 100 instruções. Resultado esperado: `r0=10`, `PC=0x080000D0`, CPSR `0x6000001F`. A fixture não acessa memória mapeada, BIOS ou periféricos e não representa cobertura de jogo comercial.

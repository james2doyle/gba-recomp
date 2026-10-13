# Experiência proposta

Os comandos abaixo descrevem o fluxo de produto futuro. O scaffold atual só implementa a fixture interna `emit-fixture` do GBA-010; os comandos de produto ainda não existem.

```sh
cargo run -p gba-recomp -- emit-fixture --out ./target/fixture.o --opt O0

gba-recomp analyze ./game.gba --report ./analysis.json
gba-recomp profile validate ./game.toml --rom ./game.gba
gba-recomp build ./game.gba --profile ./game.toml --target aarch64-apple-darwin --mode strict-aot --out ./build/game
gba-recomp verify ./build/game/manifest.json --scenario ./scenario.json
gba-recomp package ./build/game/manifest.json --out ./dist
```

O usuário seleciona a ROM; a ferramenta identifica a revisão pelo hash, mostra o perfil compatível e gera um relatório de cobertura e pendências. Ao compilar, mostra destino, modo AOT e dependências ausentes. O executável contém menu acessível por teclado e controle, sem exigir LLVM instalado.

No menu, **Vídeo** ajusta janela/tela cheia, escala, proporção, filtro, correção de cores, persistência LCD e sincronização. **Áudio** ajusta dispositivo, volume, mute e latência. **Controles** remapeia teclado/gamepad, eixos e atalhos. Cada página permite aplicar, cancelar e restaurar; informa se usa preferência global ou específica do jogo.

Configurações de compatibilidade e opções que exigem recompilação aparecem em uma página avançada separada, com origem e diagnóstico. O menu não promete aplicar uma otimização de compilação ao executável atual.

Ao abrir o menu, o jogo pausa por padrão; a saída de áudio é drenada e retomada sem disparar eventos guest atrasados. Perda de dispositivo preserva bindings e oferece recuperação. Alterações visuais não mudam save, contagem de ciclos ou hash do framebuffer bruto.

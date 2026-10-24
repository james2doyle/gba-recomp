# ADR-043 — Gamepad virtual como navegação do menu em tela touch

- Estado: Accepted
- Data: 2026-10-24
- Supersede: —

## Contexto

A TOUCH-68-01 criou o gamepad virtual: o frontend desenha direcional, A, B, L, R, Start e Select sobre o frame e converte cada toque em bits de `KEYINPUT` (SPEC-068). A abertura do menu em tela touch veio depois, em `79aa4a8` (TOUCH-68-02 original e MENU-63-01): pressão longa de 500 ms numa área livre, mais o botão voltar do Android.

Ficou faltando o meio do caminho. Com o menu aberto, `app.rs` entra no ramo suspenso, que só redesenha o frame com `draw_menu` — o gamepad virtual deixa de ser desenhado — e nenhum evento de dedo vira `Nav`. O `FingerDown` só arma a pressão longa quando `!menu.open`, e o `FingerMotion`/`FingerUp` só alimentam o gesto. O único controle que funciona com o menu aberto é o botão voltar, porque a SDL o entrega como `SDL_SCANCODE_AC_BACK` e o teclado mapeia para `Nav::Back`. Em aparelho, o resultado foi: o menu abre e não navega (KNO-009, EV-009-16).

Quatro caminhos foram levantados: (a) reusar o gamepad virtual, (b) tocar na linha desejada, (c) gestos de arrasto e (d) uma faixa de navegação desenhada dentro do menu.

## Decisão

1. Enquanto o menu está aberto, o gamepad virtual vira o dispositivo de navegação: direcional → `Nav::Up/Down/Left/Right`, `A` → `Nav::Confirm`, `B` → `Nav::Back`, `Start`/`Select` → última/primeira linha. O mapeamento é uma função pura em `touch.rs` (`menu_key`), testável sem dispositivo.
2. A navegação é por **borda de pressão**: o `Nav` é emitido uma vez, ao tocar, e não repete enquanto o dedo fica parado. Repetição automática num direcional de 30 px de raio é armadilha — encostar para ler a lista percorreria tudo e passaria por "Sair".
3. O gamepad continua sendo desenhado sob o menu, esmaecido, em vez de sumir: o alvo do toque precisa estar visível. O desenho continua condicionado a `input.touch_controls` (TOUCH-68-01), então `off` desliga também a navegação por toque.

## Alternativas

- **Tocar na linha desejada.** É o gesto mais direto, mas a lista tem passo de 9 px lógicos (~2,5 mm num aparelho de 1080 px de largura) contra uma ponta de dedo de ~9 mm; exigiria confirmação em dois toques e mover a geometria de `draw_menu` para `menu.rs` para não duplicá-la. Fica como incremento futuro, não como substituto: continua sem resolver `touch_controls = off`.
- **Gestos (arrastar para percorrer, tocar para confirmar).** Não acrescenta elementos na tela e escala para a página Controles (25 linhas), mas é o menos descobrível de todos e colide com a pressão longa que abre o menu. Os limiares só se julgam em aparelho.
- **Faixa de navegação dentro do menu.** É a única opção que funciona com `touch_controls = off` e não depende do gamepad, mas consome 2–3 linhas visíveis das 15 e exige repetição automática (sem ela, 25 linhas viram 25 toques). Custo maior para um controle que já existe na tela.

## Consequências

- Nenhum desenho novo: `draw_touch` é reaproveitado, com alfa reduzido sob o menu.
- `touch_controls = off` passa a ter uma limitação explícita: o menu abre (pressão longa e botão voltar continuam valendo) e navega pelo botão voltar, mas as setas e o `A` não têm como ser acionados por toque. Registrado em KNO-009 e em `docs/building-an-apk.md`.
- O remapeamento (`menu.remap`) não muda: enquanto ele espera uma tecla, os eventos de teclado já são consumidos antes da navegação, e o toque não é uma fonte de binding.
- O guest continua suspenso: o mapeamento de toque para `KEYINPUT` só roda no ramo normal do loop, então navegar o menu nunca escreve em `KEYINPUT`.

## Critérios para revisitar

Se o teste em aparelho mostrar que tocar o direcional com o menu aberto erra o alvo (alvos de 30 px sob o overlay são menores do que parecem quando o dedo cobre a tela), ou se aparecer um caminho de tela touch sem gamepad virtual — iOS ou Web com `touch_controls = off` —, implementar a faixa de navegação (alternativa 4) como complemento, não como troca.

## Referências

[SPEC-068](../specs/SPEC-068-multiplatform-targets.md) (TOUCH-68-01, TOUCH-68-02), [SPEC-063](../specs/SPEC-063-recompiled-frontend-settings.md) (MENU-63-01), [KNO-009](../knowledge/KNO-009-platform-android.md), [ADR-041](ADR-041-interpreter-first-core.md).

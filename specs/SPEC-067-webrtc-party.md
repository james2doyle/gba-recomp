# SPEC-067 — Party em rede por WebRTC: sala, chat de texto, voz, telas e cabo link

Estado do documento: Accepted em 2026-10-14. **Fase 1 implementada**: crate `gba-net` (webrtc 0.21.1 + tokio; sinalização TCP/JSON embutida no anfitrião; estrela com canal `chat` repassado e nome carimbado pelo anfitrião) e app `gba-party` (egui) aberto por "Party em rede…" no menu do app, no menu de contexto e incluído no `.app`. Verificado: teste com anfitrião + 2 convidados em loopback por WebRTC real (entrada, chat nos dois sentidos, repasse, saída) e recusa de código errado; app aberto a partir do `.app` sem erro. Pendente: uso pela internet/NAT real, fases 2–4. Progresso: [ROADMAP](../ROADMAP.md).

## Problema e escopo

Jogar junto pela rede: conversar por texto e voz, ver as telas uns dos outros (party) e, por último, emular o cabo link do GBA entre instâncias para trocas e batalhas. WebRTC fornece criptografia (DTLS), travessia de NAT (ICE/STUN/TURN) e canais de dados; não há servidor central obrigatório.

## Decisões

- Biblioteca `webrtc` (webrtc-rs, Rust, runtime tokio) e Opus via libopus para voz; dependências fora de `gba-core` e do runtime determinístico do jogo.
- Topologia estrela, até 4 participantes: cada convidado tem uma `RTCPeerConnection` com o anfitrião, que repassa chat, voz e telas aos demais.
- Sinalização embutida: o app do anfitrião abre, numa thread secundária, um servidor TCP (linhas JSON) protegido por código de sala; convidados conectam em `host:porta` + código. Pela internet o anfitrião precisa de porta acessível (redirecionamento, VPN); na LAN funciona direto. STUN público configurável; TURN opcional.
- O app companheiro `gba-party` (egui) hospeda a sessão e a interface (participantes, chat, voz, telas) e é aberto pelo jogo ("Party…"); a comunicação jogo↔party para telas e cabo link é IPC local (fases 3–4).

## Requisitos normativos

- NET-67-01 (fase 1): hospedar com nome, porta (padrão 7373) e código gerado de 6 letras; entrar com endereço, código e nome; lista de participantes com estado da conexão; chat de texto confiável e ordenado (data channel `chat`), com histórico na sessão. Código errado, sala cheia ou anfitrião inacessível MUST gerar mensagem clara.
- NET-67-02 (fase 2): voz com captura do microfone, Opus 48 kHz mono 20 ms, data channel não confiável `voice`, mute local e por participante, indicador de quem fala; o anfitrião repassa pacotes sem mixar.
- NET-67-03 (fase 3): opcionalmente transmitir a tela do jogo (framebuffer 240×160, compressão delta+RLE, 15–30 fps configurável) pelo data channel não confiável `screen`; a party mostra as telas em grade. Transmitir tela nunca altera a simulação (SET-63-02).
- NET-67-04 (fase 4): emulação do cabo link (SIO multiplayer 16 bits, 2–4 jogadores) em lockstep pelo data channel confiável `link`, com latência medida e espera do guest documentada; requer contrato SIO no `gba-core` (hoje "desconectado").

## Critérios de aceite

Fase 1: teste automatizado com dois processos/sessões na mesma máquina (anfitrião + convidado) trocando mensagens; erros de código e sala cheia; smoke da janela `gba-party`. Fases seguintes definem seus testes ao serem iniciadas.

## Referências

[SPEC-063](SPEC-063-recompiled-frontend-settings.md), [SPEC-064](SPEC-064-settings-app-input-devices.md), [SPEC-007](SPEC-007-bios-cart-state.md) CART-02 (serial/link).

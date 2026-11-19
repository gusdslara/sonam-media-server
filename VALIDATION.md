# Validação local

Execução em Windows, em 12 de novembro de 2026. Código e arquivos novos em `C:\dev\sonam`; nenhuma alteração no CtrlB.

## Interface

- `npm run check`: zero erros e zero avisos Svelte/TypeScript.
- `npm run build`: bundle estático gerado. Aproximadamente 49 kB gzip de JavaScript e 8 kB gzip de CSS.
- Inspeção visual pelo navegador em desktop e viewport 390 × 844: primeiro acesso, biblioteca, navegação de músicas e player de áudio. Um arquivo WAV sintético foi reproduzido.
- Os dados de QA foram criados em biblioteca temporária, separada de `data/`. Não foi cadastrado administrador na biblioteca real do projeto.
- Não foi usado conteúdo pessoal para os testes.

## Clientes e ferramentas

- Android TV/Fire TV: `assembleDebug` com Gradle Wrapper 8.11.1, JDK 21 e SDK 36: compilação bem-sucedida. APK de teste em `artifacts/sonam-android-tv-debug.apk`.
- Roku: `npm run roku:build`: parser e validação BrightScript concluídos sem erro, ZIP gerado em `artifacts/sonam-roku.zip`.
- `cargo check` do Tauri passou antes da adição do decodificador de miniaturas. A compilação final é registrada abaixo.
- Os clientes de TV não foram instalados em aparelhos físicos. Compilação não equivale a certificação nem garante suporte a todos os codecs/WebViews.
- Docker, systemd, Linux e iOS não foram executados nesta máquina.
- Auditoria npm de produção: zero vulnerabilidades conhecidas no momento da consulta. Auditoria completa: três avisos de baixa severidade na cadeia de desenvolvimento de `deepmerge`/plugin Svelte, sem correção indicada pelo audit. Isso não substitui auditoria do produto.

## Testes do servidor

Quatro testes Rust cobrem assinatura de mídia, precedência de bloqueios, rejeição de acesso não autenticado/Host estranho e persistência somente do hash de sessão. Passaram na execução inicial.

O teste HTTP `scripts/smoke.py` executa uma instância descartável na porta 18096, cobrindo:

- primeiro acesso único, login, sessão, logout;
- upload acima do limite pequeno padrão de formulários, rejeição de HTML disfarçado;
- leitura Range parcial e 416 fora do tamanho;
- criação de álbum e validação de tipo;
- restrição por gênero sem diferença entre maiúsculas/minúsculas e por coleção;
- ausência de resultados e bloqueio de URL direta, conversão, miniatura e progresso;
- impedimento de upload/administrador para perfil sem permissão;
- progresso por conta;
- PIN aprovado por conta autenticada, resgate único, sessão com as mesmas restrições;
- persistência da preferência GPU;
- bloqueio de Origin/Host externos.

Esses fluxos passaram antes da adição de miniaturas. A versão final também verifica JPEG de miniatura/cache e, quando há FFmpeg, produz um vídeo sintético e decodifica as saídas de conversão CPU e preferência GPU/fallback.

## Limitação de ambiente encontrada

Uma compilação simultânea com símbolos completos esgotou o espaço livre do volume C:. Foram removidos apenas os diretórios `server/target` e `src-tauri/target`, criados nesta tarefa. Perfis de desenvolvimento/teste agora desativam símbolos e compilação incremental e usam dois jobs por padrão. O código-fonte, os dados e arquivos externos ao projeto foram preservados. O build final Tauri foi executado com override de oito jobs para reduzir o tempo de compilação.

## Resultados finais do servidor

- `cargo test --manifest-path server/Cargo.toml --offline`: **4 passaram, 0 falharam**, incluindo a versão com miniaturas.
- `cargo build --manifest-path server/Cargo.toml --offline`: executável gerado com frontend embutido. Cópia portátil de desenvolvimento em `artifacts/sonam-server-windows.exe` (~33 MB); não requer Node/Rust para executar.
- `scripts/smoke.py` final: **passou**, incluindo geração e cache JPEG de miniatura, upload de WAV maior que 32 KiB, política também na rota de miniaturas e todos os fluxos HTTP descritos acima.
- Conversão real testada usando uma cópia portátil FFmpeg 9.0.2 do fornecedor Gyan, listado no site oficial FFmpeg; SHA-256 do arquivo baixado conferido com o fornecedor. Nenhum FFmpeg foi instalado no sistema.
- O teste criou um vídeo sintético, solicitou conversão com `cpu=true` e com `cpu=false`/preferência GPU e decodificou integralmente ambas as saídas usando FFmpeg: **passou**. O segundo caminho permite fallback CPU; este resultado não comprova uso de GPU física ou performance em outros hardwares.
- O processo e a biblioteca temporária foram encerrados ao concluir o teste. Nenhum usuário ou arquivo sintético foi colocado na biblioteca real.

## Resultado final Tauri / entrega

- `npm run desktop:build -- --debug --bundles nsis`: **passou**, gerando `Sonam_0.1.0_x64-setup.exe` (~10,57 MiB).
- A falha MinGW `export ordinal too large` foi resolvida com `rustc-link-arg-cdylib=--exclude-all-symbols` somente para o target Windows GNU, emitido por `src-tauri/build.rs`. A configuração de exportação mobile não foi alterada.
- O teste de política também foi ampliado para gêneros com acentos (`AÇÃO`/`ação`); os quatro testes passaram novamente. O instalador e o servidor portátil foram recompilados com essa correção.
- Instalador, servidor portátil, APK, ZIP Roku, ZIP desktop e hashes SHA-256 foram consolidados em `artifacts/`.
- A abertura do aplicativo nativo pelo controle do computador não pôde ser validada: a ferramenta aguardou aprovação para executar o app e a aprovação expirou. Isso não é aprovação de teste de inicialização; teste manual do Tauri continua pendente. Nenhum instalador foi executado.
- A inspeção web e reprodução WAV foram realizadas; elas não substituem teste da janela Tauri.
- O cache `src-tauri/target` foi removido depois da cópia dos pacotes para recuperar espaço. As fontes e saídas de entrega foram preservadas; uma próxima compilação Tauri reconstruirá o cache.
- O servidor portátil final foi iniciado em `http://127.0.0.1:8096`, usando a biblioteca real ainda sem contas. O teste de QA deixou de executar. Para usar o aplicativo Tauri na mesma porta, encerre o servidor portátil antes de abri-lo.

## Atualização de 14 de novembro de 2026: celular servidor, unidade de rede e endereços

- `cargo test` (servidor): **11 passaram, 0 falharam**, incluindo pasta monitorada (inclusão, gênero por pasta, mover mantendo o ID, remoção), rejeição de `..` em caminhos, nomes `.local`, lista de hosts permitidos, parser do link Cloudflare e garantia de que requisições pelo túnel não contam como locais.
- `scripts/smoke.py`: **passou**, agora com WebDAV (401 sem senha, 403 para perfil sem envio, PROPFIND, MKCOL, PUT, MOVE), arquivo HTML da unidade servido com CSP `sandbox`, mídia da pasta Biblioteca aparecendo na biblioteca com gênero da pasta, MOVE preservando o item, exclusão pela interface removendo o arquivo da unidade, assistente do Windows recusando endereço desconhecido e host `.local` aceito.
- Resolução real de `sonam.local` pelo Windows 11 desta máquina (mDNS): resolveu para o IP do Wi-Fi.
- Link para fora de casa: `cloudflared` 2026.10.3 gerou um endereço `trycloudflare.com` para uma instância descartável; status, login (cookie com `Secure`) e PROPFIND WebDAV funcionaram pelo HTTPS público; desligar encerrou o processo. A primeira tentativa demorou mais que o tempo de espera do teste; as seguintes ficaram prontas em segundos. O túnel ficou ativo só durante o teste.
- Interface: `npm run check` sem erros/avisos; inspeção no navegador das Configurações (endereços, QR, nome, link externo) e da Unidade de rede em desktop e 375 px, sem rolagem horizontal; download do assistente `.cmd` com CRLF.
- Android: `scripts/android-rust.py` compilou `libsonam_android.so` para arm64-v8a, armeabi-v7a e x86_64 (NDK 29, API 24); as bibliotecas exportam os três símbolos JNI e dependem só de libc/libm/libdl. O build armv7 revelou um estouro de `usize` no limite de upload (64 GiB) em 32 bits, corrigido (limite de ~4 GiB nesses aparelhos). Teste de host da ponte JNI: iniciar, responder `/api/status`, parar liberando a porta e iniciar de novo: **passou**. `gradlew assembleDebug` (JDK 21 do Android Studio): APK gerado em `artifacts/sonam-android-server-debug.apk`.
- Aparelho físico, com autorização do dono: Galaxy S23 (SM-S911B, Android 16, arm64) por adb sem fio. O APK instalou, a biblioteca Rust carregou, o serviço ficou em primeiro plano (`types=0x40000000`, specialUse) com wake lock parcial, Wi-Fi lock e multicast lock ativos. Do PC pelo Wi-Fi: `/api/status`, WebDAV (401 com desafio Basic) e OPTIONS `/` responderam; com o app em segundo plano o servidor continuou respondendo. O Windows resolveu `sonam.local` para o IP do celular e `curl` abriu o servidor pelo nome. A conta foi criada pelo dono no próprio app; os testes não criaram contas nem arquivos nesse aparelho.
- **Não validado:** uso prolongado de horas/dias, reinício do celular com início automático e economia de bateria da Samsung. O mapeamento real pelo WebClient do Windows, Finder e GVFS não foi executado. O app Tauri foi adaptado ao novo ciclo de vida, mas não recompilado: o disco C: ficou com menos de 1 GB livre; os caches de build Android foram apagados para liberar espaço.

## Atualização de 14 de novembro de 2026 (noite): fotos, primeiro acesso e app Android

- Fotos: novo endpoint `/api/media/{id}/details` (mesmas regras de visibilidade do arquivo) com EXIF via `kamadak-exif`. Fotos sintéticas geradas localmente (sem conteúdo pessoal) com câmera, lente, exposição, GPS e orientação 6: data/fuso, câmera, ƒ/1,85, 1/120 s, ISO 100, 6,8 mm (24 mm equiv.), 800 × 1200 (dimensões já giradas) e 22°57′06″ S, 43°12′37″ O com 12 m. Miniatura da foto girada saiu vertical (427 × 640). Miniaturas antigas, sem rotação, ficam ignoradas (novo nome de cache `ID-o.jpg`).
- Visualizador no navegador, desktop e 375 px: setas do teclado, arrastar para trocar (4 fotos), arrastar para cima abre a folha de detalhes, painel lateral no desktop.
- Primeiro acesso: no próprio servidor (loopback) não aparece campo de chave; por IP da rede aparece a explicação e o campo obrigatório. `/api/status` agora informa `local`.
- Login: “Entrar com um código” com passos numerados, código grande em dois blocos, contagem regressiva e renovação automática do código.
- Corrigido: no celular, o título da tela de entrada juntava palavras (“melhorescoleções”). Arquivos pequenos apareciam como “0.0 MB”.
- `cargo test`: 13 passaram. `scripts/smoke.py`: passou. `npm run check`: sem erros/avisos.
- Android: edge-to-edge com insets reais (barra superior não fica mais sob relógio/bateria; teclado e barra de navegação respeitados), nome `sonam.local:8096` no topo e na notificação, diálogo de endereços com copiar/compartilhar, botão “Desligar” da notificação não deixa mais a tela presa em “Iniciando…”, início no boot respeita quem desligou o servidor. O APK agora inclui só arm64-v8a e armeabi-v7a (x86_64 sob demanda com `--abi x86_64`).
- APK atualizado instalado no Galaxy S23 por cima da versão anterior (dados e conta mantidos): serviço em primeiro plano ativo e `http://sonam.local:8096` respondendo. A barra superior corrigida não pôde ser conferida por captura de tela porque o celular estava bloqueado; falta a conferência visual no aparelho.

## Atualização de 15 de novembro de 2026: tela Arquivos, pastas por tipo e nome curto

- Unidade com Fotos, Músicas, Vídeos (monitoradas) e Arquivos. Envios pelo app vão para a pasta do tipo; envios antigos de `media/ID` são migrados ao iniciar (teste de unidade: linha mantém o ID, arquivo movido, nova verificação sem mudanças). As quatro pastas principais recusam DELETE/MOVE (403).
- `/api/drive/list`: pastas primeiro, itens da biblioteca com ID, espaço livre; recusa `..` (400) e perfis sem envio (403).
- Tela Arquivos no navegador (instância descartável, fotos sintéticas): navegar Início › Fotos › Viagem, criar pasta, renomear, mover para subpasta, abrir a foto no visualizador, excluir pasta; a biblioteca acompanhou cada passo (mesmo ID após renomear/mover). 375 px sem rolagem horizontal, seis abas na barra inferior.
- Nome curto: nesta máquina Windows, `http://hs-pc/api/status` respondeu sem porta (porta 80 aberta pela instância), `hs-pc.local` também; Host `hs-pc.evil.com` recebeu 403. Domínio próprio inválido recusado (400); válido aceito e listado.
- `cargo test`: 16 passaram. `scripts/smoke.py`: passou (agora também listagem, proteção das pastas principais e envio do app dentro da unidade; roda com `SONAM_PORT80=off`).

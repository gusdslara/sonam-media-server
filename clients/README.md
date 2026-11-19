# Clientes

O servidor para celular Android fica em `android/` e está documentado no [README principal](../README.md#servidor-no-android). Os clientes abaixo apenas assistem.

## Clientes de TV

Todos usam o mesmo servidor e as mesmas restrições por perfil. Cadastre o administrador pelo servidor antes de conectar clientes. A sessão dura até 30 dias e pode expirar antes se removida localmente. Se o IP mudar, reconfigure o endereço do servidor.

## Android TV / Fire TV

`android-tv` é um launcher Android leve para a interface Svelte em modo TV. Não embute nem inicia o servidor: conecta ao IP do PC/Linux. Essa separação evita manter um servidor desnecessário na TV. O aplicativo principal desktop/mobile é o projeto Tauri da raiz.

Requer JDK 17 ou 21 e Android SDK 36. Abra a pasta no Android Studio ou use:

```sh
cd clients/android-tv
./gradlew assembleDebug
# Windows: .\gradlew.bat assembleDebug
```

O APK vai para `app/build/outputs/apk/debug/app-debug.apk`. Para teste com depuração já autorizada pelo dono do aparelho:

```sh
adb install app/build/outputs/apk/debug/app-debug.apk
```

Não instalamos nada em aparelhos automaticamente. No primeiro acesso, informe `http://192.168.x.x:8096` e escolha entrar com código. Aceita somente IPv4 privado ou loopback. Não segue navegações para outras origens, não disponibiliza ponte nativa a JavaScript e não aceita cookies de terceiros. O tráfego HTTP da LAN não é cifrado.

Para trocar servidor, segure Voltar. Use D-pad e OK para navegar; Voltar fecha o player/modal. Hardware de vídeo é gerenciado pelo WebView do fabricante. Algumas versões antigas de Fire OS/WebView podem não executar o JavaScript moderno gerado pelo Svelte/Vite; precisam ser testadas e podem exigir um cliente nativo Media3 ou um build de compatibilidade específico. Não há integração de voz, busca do sistema ou publicação em lojas nesta versão.

## iOS

`ios` é um cliente nativo para iPhone e iPad (iOS 15+) em SwiftUI + `WKWebView`, no mesmo modelo do launcher Android TV: mostra a interface Svelte servida pelo servidor da sua casa. **É só cliente.** Não embute nem inicia o servidor: o iOS suspende apps em segundo plano, então um servidor no iPhone pararia ao bloquear a tela. Deixe o servidor ligado no PC, Linux ou celular Android da mesma rede.

O que faz:

- Tela de conexão com o endereço do servidor (ex.: `http://192.168.1.10:8096`) e o botão **Usar sonam.local**, que tenta `http://sonam.local:8096`. Se digitar sem `http://`, ele é acrescentado. O último servidor fica salvo e o app abre direto nele.
- Aceita somente IPv4 privado (10.x, 172.16–31.x, 192.168.x), loopback ou nomes `.local`, sem usuário ou senha na URL. Endereços Tailscale (100.x), nomes curtos (`http://sonam`) e domínios públicos não são aceitos nesta versão; para eles use o Safari.
- Não segue navegações para outras origens: links externos abrem no Safari. Não há ponte nativa para o JavaScript. Os cookies de sessão ficam no armazenamento padrão do `WKWebView` e persistem entre aberturas (sessão de até 30 dias, como nos outros clientes).
- Puxe a página para baixo para recarregar. **Toque e segure com dois dedos** para abrir o menu (Recarregar / Trocar servidor), equivalente a segurar Voltar no Android TV. Quando o servidor não responde, aparece uma tela com **Tentar de novo** e **Trocar servidor**.
- Envio de fotos e vídeos pelo seletor nativo do iOS (fototeca, câmera ou Arquivos). Os botões **Baixar** da interface abrem a folha de compartilhamento para salvar em Arquivos ou na fototeca.
- Áudio continua com a tela bloqueada ou com o app em segundo plano (modo de áudio em segundo plano). Vídeo pausa ao sair do app, como no Safari; use Picture in Picture ou AirPlay pelo player.

No primeiro acesso o iOS pede permissão de **Rede Local**: aceite. Se a primeira tentativa falhar enquanto o aviso estava aberto, toque em **Tentar de novo**. Se negou, ative em Ajustes › Privacidade e Segurança › Rede Local › Sonam.

### Gerar o .ipa (sem Mac)

O projeto Xcode não é versionado: ele é gerado a partir de `clients/ios/project.yml` pelo [XcodeGen](https://github.com/yonaskolb/XcodeGen). O build roda no GitHub Actions (runner macOS):

1. No GitHub, abra **Actions › iOS (ipa sem assinatura) › Run workflow**.
2. Ao terminar, baixe o artefato **Sonam-unsigned-ipa** (um ZIP com `Sonam-unsigned.ipa`).

O workflow (`.github/workflows/ios.yml`) executa `xcodegen generate` e `xcodebuild ... -sdk iphoneos CODE_SIGNING_ALLOWED=NO build`, e empacota `Payload/Sonam.app` no `.ipa`. Com um Mac, o mesmo vale localmente: `brew install xcodegen && cd clients/ios && xcodegen generate && open Sonam.xcodeproj`.

### Instalar por sideload

O `.ipa` **não é assinado**: o iPhone só o instala depois de assinado com o seu Apple ID. Use uma ferramenta de sideload, por exemplo:

- **[Sideloadly](https://sideloadly.io/)** (Windows/macOS): conecte o iPhone por cabo, arraste o `.ipa`, informe seu Apple ID e clique em Start.
- **[AltStore](https://altstore.io/)**: instale o AltServer no PC e o AltStore no iPhone, depois abra o `.ipa` pelo AltStore.

Com Apple ID gratuito:

- A assinatura **expira em 7 dias**. Depois disso o app não abre até ser reassinado (o AltStore renova sozinho se o AltServer estiver rodando na mesma rede; no Sideloadly, instale de novo). Seus dados e a sessão continuam.
- No iOS 16+, ative **Ajustes › Privacidade e Segurança › Modo de Desenvolvedor** e, na primeira vez, confie no perfil em **Ajustes › Geral › VPN e Gerenciamento de Dispositivo**.
- O bundle id é `org.sonam.ios`; as ferramentas podem trocá-lo para caber nos limites da conta gratuita (até 3 apps ativos).

Limites: precisa do servidor ligado e acessível na mesma rede; o tráfego é **HTTP sem cifra na LAN** (como nos outros clientes). Não há publicação na App Store, integração com CarPlay, widgets ou downloads para assistir offline. Ainda requer validação em iPhone/iPad físico.

## Roku

Roku usa BrightScript/SceneGraph, não JavaScript ou WebView. O código nativo tem task de rede fora da thread de renderização e transmite o bearer em headers, nunca no endereço do vídeo.

```sh
npm ci
npm run roku:build
```

O ZIP fica em `artifacts/sonam-roku.zip`. Faça sideload somente pelo modo de desenvolvedor do seu próprio Roku seguindo a [documentação oficial](https://developer.roku.com/dev/docs/developer-setup). Não é um pacote assinado para loja. A biblioteca é uma lista simples de arquivos, sem o layout do cliente Svelte.

No primeiro acesso, informe o IP e autorize o PIN no celular/PC. Selecione o arquivo e pressione OK. `*` alterna original/compatível antes de iniciar a reprodução. Compatível exige FFmpeg no servidor. Voltar fecha o player ou abre a configuração do servidor. Fotos são baixadas em arquivo temporário com header de autenticação. Players recebem sessão via [metadados HTTP oficiais do Roku](https://developer.roku.com/dev/docs/content-metadata).

O cliente inicial não implementa navegação por playlists, progresso por perfil, pesquisa, modo aleatório ou atualização de biblioteca em tempo real. A disponibilidade de codecs precisa ser verificada no dispositivo físico. Não alegamos suporte certificado a modelos Roku ou Fire TV específicos.

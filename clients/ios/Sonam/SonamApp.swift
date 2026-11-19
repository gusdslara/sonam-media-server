import AVFoundation
import SwiftUI

/// Cliente iOS do Sonam: só conecta a um servidor da rede local e mostra a interface web dele.
/// Não embute servidor nem expõe ponte nativa ao JavaScript (mesmo modelo do launcher Android TV).
@main
struct SonamApp: App {
    @StateObject private var store = ServerStore()

    init() {
        AudioSessionSetup.configure()
    }

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(store)
        }
    }
}

/// Alterna entre a tela de conexão e a interface do servidor.
struct RootView: View {
    @EnvironmentObject private var store: ServerStore

    var body: some View {
        Group {
            if let origin = store.current {
                ZStack {
                    // Faixas da barra de status e do indicador de início com a cor de fundo da interface web.
                    Theme.pageBackground.ignoresSafeArea()
                    WebScreen(origin: origin, onChangeServer: { store.disconnect() })
                        .ignoresSafeArea(.keyboard)
                }
                .id(origin)
                // A interface web é clara; texto escuro na barra de status.
                .preferredColorScheme(.light)
            } else {
                ConnectView()
            }
        }
    }
}

/// Categoria .playback + UIBackgroundModes "audio": a música continua com a tela bloqueada.
/// A sessão não é ativada aqui para não interromper o áudio de outros apps ao abrir o Sonam;
/// o WebKit a ativa quando a página começa a tocar.
enum AudioSessionSetup {
    static func configure() {
        do {
            try AVAudioSession.sharedInstance().setCategory(.playback, mode: .default, options: [])
        } catch {
            print("Sonam: não foi possível configurar o áudio em segundo plano: \(error.localizedDescription)")
        }
    }
}

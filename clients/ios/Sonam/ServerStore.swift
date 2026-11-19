import Foundation

/// Guarda o servidor atual e lembra o último usado (UserDefaults), como as preferências do Android TV.
final class ServerStore: ObservableObject {
    private static let serverKey = "server"

    @Published private(set) var current: URL? = nil
    @Published private(set) var lastServer: String = ""

    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        let saved = defaults.string(forKey: Self.serverKey) ?? ""
        lastServer = saved
        // Abre direto no último servidor; se ele não responder, a tela de erro oferece trocar.
        if !saved.isEmpty, let origin = try? ServerAddress.parse(saved) {
            current = origin
        }
    }

    /// Valida, salva e conecta. Lança `AddressError` com a mensagem para o usuário.
    func connect(_ address: String) throws {
        let origin = try ServerAddress.parse(address)
        let value = origin.absoluteString
        defaults.set(value, forKey: Self.serverKey)
        lastServer = value
        current = origin
    }

    /// Volta à tela de conexão. O endereço salvo continua preenchido e os cookies de cada servidor são mantidos.
    func disconnect() {
        current = nil
    }
}

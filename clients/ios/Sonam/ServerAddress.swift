import Foundation

/// Erro de validação mostrado na tela de conexão.
struct AddressError: LocalizedError {
    let message: String

    var errorDescription: String? { message }
}

/// Validação do endereço do servidor, equivalente à do launcher Android TV:
/// aceita só http/https, sem usuário/senha, com host IPv4 privado (10/8, 172.16/12, 192.168/16),
/// loopback (127/8, localhost) ou nome mDNS terminado em `.local`. Guarda apenas a origem
/// (esquema + host + porta); caminho, query e fragmento são descartados.
enum ServerAddress {
    static let suggested = "http://sonam.local:8096"
    static let example = "http://192.168.1.10:8096"

    static func parse(_ raw: String) throws -> URL {
        var text = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        if text.isEmpty {
            throw AddressError(message: "Digite o endereço do servidor. Exemplo: \(example)")
        }
        if !text.contains("://") {
            text = "http://" + text
        }
        guard let parts = URLComponents(string: text),
              let scheme = parts.scheme?.lowercased(),
              scheme == "http" || scheme == "https",
              let rawHost = parts.host,
              !rawHost.isEmpty
        else {
            throw AddressError(message: "Endereço inválido. Use o formato \(example)")
        }
        if parts.user != nil || parts.password != nil {
            throw AddressError(message: "Não coloque usuário ou senha no endereço.")
        }
        if let port = parts.port, !(1...65535).contains(port) {
            throw AddressError(message: "Porta inválida. A porta padrão do Sonam é 8096.")
        }
        let host = rawHost.lowercased()
        guard isLocal(host) else {
            throw AddressError(message: "Use um IP da rede local (192.168.x.x, 10.x.x.x ou 172.16–31.x.x) ou um nome .local.")
        }
        var origin = URLComponents()
        origin.scheme = scheme
        origin.host = host
        origin.port = parts.port
        guard let url = origin.url else {
            throw AddressError(message: "Endereço inválido. Use o formato \(example)")
        }
        return url
    }

    static func isLocal(_ host: String) -> Bool {
        if host == "localhost" {
            return true
        }
        if let octets = ipv4Octets(host) {
            switch (octets[0], octets[1]) {
            case (127, _), (10, _), (192, 168), (172, 16...31):
                return true
            default:
                return false
            }
        }
        return isMDNSName(host)
    }

    /// Quatro números decimais de 0 a 255 separados por ponto; qualquer outra forma é rejeitada.
    static func ipv4Octets(_ host: String) -> [Int]? {
        let parts = host.split(separator: ".", omittingEmptySubsequences: false)
        guard parts.count == 4 else {
            return nil
        }
        var octets: [Int] = []
        for part in parts {
            guard (1...3).contains(part.count),
                  part.allSatisfy({ $0.isASCII && $0.isNumber }),
                  let value = Int(part),
                  value <= 255
            else {
                return nil
            }
            octets.append(value)
        }
        return octets
    }

    /// Nome como `sonam.local` ou `casa-da-vo.local` (letras, números e hífen por rótulo).
    static func isMDNSName(_ host: String) -> Bool {
        guard host.hasSuffix(".local"), host.count <= 253 else {
            return false
        }
        let labels = host.split(separator: ".", omittingEmptySubsequences: false)
        guard labels.count >= 2 else {
            return false
        }
        return labels.allSatisfy { label in
            !label.isEmpty
                && label.count <= 63
                && !label.hasPrefix("-")
                && !label.hasSuffix("-")
                && label.allSatisfy { $0 == "-" || ($0.isASCII && ($0.isLetter || $0.isNumber)) }
        }
    }
}

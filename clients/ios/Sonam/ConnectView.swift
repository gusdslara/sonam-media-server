import SwiftUI

/// Primeira tela: endereço do servidor da rede local.
struct ConnectView: View {
    @EnvironmentObject private var store: ServerStore
    @State private var address: String = ""
    @State private var errorMessage: String? = nil
    @FocusState private var addressFocused: Bool

    var body: some View {
        ZStack {
            Theme.brandDark.ignoresSafeArea()
            ScrollView {
                VStack(alignment: .leading, spacing: Theme.Space.l) {
                    header
                    instructions
                    addressField
                    actions
                    Text("Para trocar de servidor depois, toque e segure a tela com dois dedos.")
                        .font(.footnote)
                        .foregroundColor(Theme.mutedOnDark)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .frame(maxWidth: 480, alignment: .leading)
                .padding(.horizontal, Theme.Space.l)
                .padding(.vertical, Theme.Space.xl)
                .frame(maxWidth: .infinity)
            }
        }
        .preferredColorScheme(.dark)
        .onAppear {
            if address.isEmpty {
                address = store.lastServer
            }
        }
    }

    private var header: some View {
        HStack(spacing: Theme.Space.m) {
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .fill(Theme.accent)
                .frame(width: 56, height: 56)
                .overlay(
                    Image(systemName: "play.fill")
                        .font(.system(size: 22, weight: .semibold))
                        .foregroundColor(Theme.cream)
                )
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text("Sonam")
                    .font(.largeTitle.bold())
                    .foregroundColor(Theme.cream)
                    .accessibilityAddTraits(.isHeader)
                Text("Sua casa, sua coleção")
                    .font(.subheadline)
                    .foregroundColor(Theme.mutedOnDark)
            }
        }
    }

    private var instructions: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Text("Digite o endereço que aparece no Sonam do seu PC ou celular servidor.")
            Text("Depois, entre com sua conta ou escolha entrar com código. O servidor precisa estar ligado e na mesma rede Wi‑Fi.")
        }
        .font(.body)
        .foregroundColor(Theme.mutedOnDark)
        .fixedSize(horizontal: false, vertical: true)
    }

    private var addressField: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Text("Endereço do servidor")
                .font(.subheadline.weight(.semibold))
                .foregroundColor(Theme.cream)
            TextField(ServerAddress.example, text: $address)
                .keyboardType(.URL)
                .textContentType(.URL)
                .textInputAutocapitalization(.never)
                .disableAutocorrection(true)
                .submitLabel(.go)
                .focused($addressFocused)
                .onSubmit { connect(address) }
                .font(.system(.body, design: .monospaced))
                .foregroundColor(Theme.cream)
                .padding(.horizontal, 14)
                .frame(minHeight: Theme.controlHeight)
                .background(
                    RoundedRectangle(cornerRadius: Theme.radius, style: .continuous)
                        .fill(Theme.field)
                )
                .overlay(
                    RoundedRectangle(cornerRadius: Theme.radius, style: .continuous)
                        .stroke(fieldBorderColor, lineWidth: addressFocused || errorMessage != nil ? 2 : 1)
                )
                .accessibilityLabel("Endereço do servidor")
                .accessibilityHint("Exemplo: \(ServerAddress.example)")
            if let errorMessage = errorMessage {
                Text(errorMessage)
                    .font(.footnote)
                    .foregroundColor(Theme.errorOnDark)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityLabel("Erro: \(errorMessage)")
            }
        }
    }

    private var fieldBorderColor: Color {
        if errorMessage != nil {
            return Theme.errorOnDark
        }
        return addressFocused ? Theme.cream : Theme.border
    }

    private var actions: some View {
        VStack(spacing: Theme.Space.s) {
            Button {
                connect(address)
            } label: {
                Text("Conectar")
                    .font(.headline)
                    .frame(maxWidth: .infinity, minHeight: Theme.controlHeight)
            }
            .buttonStyle(FilledButtonStyle())

            Button {
                address = ServerAddress.suggested
                connect(ServerAddress.suggested)
            } label: {
                Text("Usar sonam.local")
                    .font(.body.weight(.semibold))
                    .frame(maxWidth: .infinity, minHeight: Theme.controlHeight)
            }
            .buttonStyle(OutlineButtonStyle())
            .accessibilityHint("Tenta conectar a \(ServerAddress.suggested)")
        }
    }

    private func connect(_ value: String) {
        do {
            try store.connect(value)
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
            addressFocused = true
        }
    }
}

/// Botão principal: creme sobre o fundo escuro, como o triângulo do ícone.
private struct FilledButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .foregroundColor(Theme.brandDark)
            .background(
                RoundedRectangle(cornerRadius: Theme.radius, style: .continuous)
                    .fill(Theme.cream)
            )
            .opacity(configuration.isPressed ? 0.8 : 1)
            .contentShape(Rectangle())
    }
}

/// Botão secundário: contorno creme.
private struct OutlineButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .foregroundColor(Theme.cream)
            .background(
                RoundedRectangle(cornerRadius: Theme.radius, style: .continuous)
                    .fill(configuration.isPressed ? Theme.field : Color.clear)
            )
            .overlay(
                RoundedRectangle(cornerRadius: Theme.radius, style: .continuous)
                    .stroke(Theme.border, lineWidth: 1)
            )
            .contentShape(Rectangle())
    }
}

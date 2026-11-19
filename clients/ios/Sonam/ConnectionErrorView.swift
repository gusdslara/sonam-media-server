import UIKit

/// Mostrada por cima do WKWebView quando o servidor não responde.
final class ConnectionErrorView: UIView {
    var onRetry: () -> Void = {}
    var onChangeServer: () -> Void = {}

    private let titleLabel = UILabel()
    private let messageLabel = UILabel()
    private let detailLabel = UILabel()

    override init(frame: CGRect) {
        super.init(frame: frame)
        setUp()
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) não é usado")
    }

    func show(address: String, detail: String) {
        messageLabel.text = "O servidor \(address) não respondeu. Confira se ele está ligado e se este aparelho está na mesma rede Wi‑Fi."
        detailLabel.text = detail
        isHidden = false
        UIAccessibility.post(notification: .screenChanged, argument: titleLabel)
    }

    private func setUp() {
        backgroundColor = Theme.pageBackgroundUI

        let icon = UIImageView(image: UIImage(systemName: "wifi.exclamationmark"))
        icon.preferredSymbolConfiguration = UIImage.SymbolConfiguration(pointSize: 44, weight: .regular)
        icon.tintColor = Theme.accentUI
        icon.contentMode = .scaleAspectFit
        icon.isAccessibilityElement = false

        titleLabel.text = "Não foi possível conectar"
        titleLabel.font = UIFont.preferredFont(forTextStyle: .title2)
        titleLabel.textColor = Theme.textUI
        titleLabel.accessibilityTraits = .header
        configureMultiline(titleLabel)

        messageLabel.font = UIFont.preferredFont(forTextStyle: .body)
        messageLabel.textColor = Theme.textUI
        configureMultiline(messageLabel)

        detailLabel.font = UIFont.preferredFont(forTextStyle: .footnote)
        detailLabel.textColor = Theme.mutedUI
        configureMultiline(detailLabel)

        var retryConfig = UIButton.Configuration.filled()
        retryConfig.title = "Tentar de novo"
        retryConfig.baseBackgroundColor = Theme.accentUI
        retryConfig.baseForegroundColor = .white
        retryConfig.cornerStyle = .large
        retryConfig.buttonSize = .large
        let retryButton = UIButton(configuration: retryConfig, primaryAction: UIAction { [weak self] _ in
            self?.onRetry()
        })

        var changeConfig = UIButton.Configuration.plain()
        changeConfig.title = "Trocar servidor"
        changeConfig.baseForegroundColor = Theme.accentUI
        changeConfig.buttonSize = .large
        let changeButton = UIButton(configuration: changeConfig, primaryAction: UIAction { [weak self] _ in
            self?.onChangeServer()
        })

        for button in [retryButton, changeButton] {
            button.heightAnchor.constraint(greaterThanOrEqualToConstant: 44).isActive = true
        }

        let stack = UIStackView(arrangedSubviews: [icon, titleLabel, messageLabel, detailLabel, retryButton, changeButton])
        stack.axis = .vertical
        stack.alignment = .fill
        stack.spacing = 12
        stack.setCustomSpacing(24, after: detailLabel)
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)

        let preferredWidth = stack.widthAnchor.constraint(equalToConstant: 420)
        preferredWidth.priority = .defaultHigh
        let centerY = stack.centerYAnchor.constraint(equalTo: safeAreaLayoutGuide.centerYAnchor)
        centerY.priority = .defaultHigh
        NSLayoutConstraint.activate([
            stack.centerXAnchor.constraint(equalTo: centerXAnchor),
            centerY,
            preferredWidth,
            stack.leadingAnchor.constraint(greaterThanOrEqualTo: safeAreaLayoutGuide.leadingAnchor, constant: 24),
            stack.trailingAnchor.constraint(lessThanOrEqualTo: safeAreaLayoutGuide.trailingAnchor, constant: -24),
            stack.topAnchor.constraint(greaterThanOrEqualTo: safeAreaLayoutGuide.topAnchor, constant: 16),
        ])
    }

    private func configureMultiline(_ label: UILabel) {
        label.numberOfLines = 0
        label.textAlignment = .center
        label.adjustsFontForContentSizeCategory = true
    }
}

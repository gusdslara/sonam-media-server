import SwiftUI
import UIKit

/// Tokens visuais do app. Cores da marca vêm de `src/style.css` (--dark, --green) e do ícone.
/// Contrastes conferidos (WCAG): texto creme sobre --dark 12,8:1; texto secundário 7,6:1;
/// branco sobre --green 7,5:1; texto secundário sobre o fundo claro da web 5,6:1.
enum Theme {
    // Tela de conexão (sempre escura, cor da marca).
    static let brandDarkUI = UIColor(rgb: 0x17251E)
    static let fieldUI = UIColor(rgb: 0x22342B)
    static let borderUI = UIColor(rgb: 0x3E5247)
    static let creamUI = UIColor(rgb: 0xE5E8D9)
    static let mutedOnDarkUI = UIColor(rgb: 0xA9B6AD)
    static let errorOnDarkUI = UIColor(rgb: 0xFFB4A8)

    // Interface web (clara) e tela de erro de conexão.
    static let pageBackgroundUI = UIColor(rgb: 0xF8F9F6)
    static let textUI = UIColor(rgb: 0x25332C)
    static let mutedUI = UIColor(rgb: 0x5C675F)
    static let accentUI = UIColor(rgb: 0x355D49)

    static let brandDark = Color(uiColor: brandDarkUI)
    static let field = Color(uiColor: fieldUI)
    static let border = Color(uiColor: borderUI)
    static let cream = Color(uiColor: creamUI)
    static let mutedOnDark = Color(uiColor: mutedOnDarkUI)
    static let errorOnDark = Color(uiColor: errorOnDarkUI)
    static let pageBackground = Color(uiColor: pageBackgroundUI)
    static let accent = Color(uiColor: accentUI)

    // Escala de espaçamento 4/8 e raio padrão.
    enum Space {
        static let xs: CGFloat = 4
        static let s: CGFloat = 8
        static let m: CGFloat = 16
        static let l: CGFloat = 24
        static let xl: CGFloat = 32
    }

    static let radius: CGFloat = 12
    static let controlHeight: CGFloat = 52
}

extension UIColor {
    convenience init(rgb: UInt32) {
        self.init(
            red: CGFloat((rgb >> 16) & 0xFF) / 255,
            green: CGFloat((rgb >> 8) & 0xFF) / 255,
            blue: CGFloat(rgb & 0xFF) / 255,
            alpha: 1
        )
    }
}

import AppKit
import CoreGraphics
import Foundation
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
let selected = windows.filter { ($0[kCGWindowOwnerName as String] as? String) == "Stickies" }.map { window -> [String: Any] in
    var result: [String: Any] = [:]
    for key in [kCGWindowNumber, kCGWindowBounds, kCGWindowAlpha, kCGWindowLayer, kCGWindowSharingState] {
        if let value = window[key as String] { result[key as String] = value }
    }
    return result
}
let screens = NSScreen.screens.map { screen -> [String: Any] in
    ["scale": screen.backingScaleFactor, "frame": NSStringFromRect(screen.frame)]
}
let result: [String: Any] = ["windows": selected, "screens": screens, "method": "Public CGWindowListCopyWindowInfo metadata only; no pixel capture or note-title/content access."]
let data = try JSONSerialization.data(withJSONObject: result, options: [.prettyPrinted, .sortedKeys])
print(String(data: data, encoding: .utf8)!)

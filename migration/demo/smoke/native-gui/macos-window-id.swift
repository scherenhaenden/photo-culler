import CoreGraphics
import Foundation

guard CommandLine.arguments.count == 2,
      let pid = Int32(CommandLine.arguments[1])
else {
    fputs("expected one desktop PID\n", stderr)
    exit(2)
}

let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
let rows = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] ?? []
let candidates = rows.compactMap { row -> (id: UInt32, area: Double)? in
    guard
        (row[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value == pid,
        (row[kCGWindowLayer as String] as? NSNumber)?.intValue == 0,
        let id = (row[kCGWindowNumber as String] as? NSNumber)?.uint32Value,
        let bounds = row[kCGWindowBounds as String] as? [String: Any],
        let width = (bounds["Width"] as? NSNumber)?.doubleValue,
        let height = (bounds["Height"] as? NSNumber)?.doubleValue,
        width >= 1000,
        height >= 650
    else {
        return nil
    }
    return (id, width * height)
}

guard let window = candidates.max(by: { $0.area < $1.area }) else {
    fputs("no visible Slint-sized top-level window found for PID \(pid)\n", stderr)
    exit(1)
}
print(window.id)

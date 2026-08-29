import Foundation

struct Point {
    var x: Int
    let y: Int
    var sum: Int { return x + y }
    static let origin = Point(x: 0, y: 0)
    init(x: Int, y: Int) {
        self.x = x
        self.y = y
    }
    func moved() -> Point { return self }
}

class Box {
    var item: String = ""
    init() {}
    deinit {}
}

class Container: Box {
    var count: Int = 0
}

enum Mode: Int {
    case fast = 1
    case slow = 2
    var label: String { return "m" }
}

func localScope() -> Int {
    let hidden = 42
    return hidden
}

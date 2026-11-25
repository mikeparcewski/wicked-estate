protocol Grows {
    func grow() -> Int
}

class Plant {
    func water() -> String { return "plant" }
}

class Tree: Plant, Grows {
    override func water() -> String { return "tree" }
    func grow() -> Int { return 2 }
}

extension Tree {
    func prune() -> Tree {
        let t = Tree()
        _ = t.grow()
        return t
    }
}

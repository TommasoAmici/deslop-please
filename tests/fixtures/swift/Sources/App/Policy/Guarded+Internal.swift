extension Guarded {
    func touch() {
        value += 1
    }

    var doubled: Int { value * 2 }
    var quadrupled: Int { doubled * 2 }
}

func leak(_ guarded: Guarded) {
    guarded.other += 1
}

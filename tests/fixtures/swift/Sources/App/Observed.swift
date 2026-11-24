import Observation

@Observable
final class Observed {
    var value = 0
    func bump() { value += 1 }
}

extension Observed {
    var isSet: Bool { value != 0 }
}

final class Counter {
    var value = 0
}

extension Counter {
    var doubled: Int { value * 2 }
}

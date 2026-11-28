struct Config {
    let name: String
    var count = 0
    let values: [Int] = []

    mutating func fill(from other: Config) {
        count = other.count
    }
}

protocol Labeled {
    var label: String { get }
}

final class Tagged: Labeled {
    let label: String

    init(label: String) {
        self.label = label
    }
}

final class Failure {
    var message: String? { nil }
}

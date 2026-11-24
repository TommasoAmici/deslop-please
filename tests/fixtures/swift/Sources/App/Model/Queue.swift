final class Queue {
    var items: [Int] = []
    let limit: Int

    init(items: [Int] = [], limit: Int) {
        self.items = items
        self.limit = limit
    }

    func push(_ value: Int) {
        items.append(value)
    }

    func count() -> Int {
        items.count
    }
}

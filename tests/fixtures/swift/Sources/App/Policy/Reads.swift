func reads(_ config: Config, _ tagged: Tagged) -> Bool {
    let upper = config.name.uppercased()
    let first = config.values[0]
    let label = tagged.label.lowercased()
    return upper.isEmpty || first == 0 && label.isEmpty
}

func readsLabeled(_ item: Labeled) -> String {
    item.label
}

func hasFailure(_ failure: Failure) -> Bool {
    failure.message != nil
}

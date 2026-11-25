func runLow() -> Int {
    let queue = Queue(limit: 10)
    admit(queue)
    return next(queue)
}

func runHigh() -> Int {
    let queue = Queue(limit: 10)
    admit(queue)
    return next(queue)
}

func runBoth() -> Int {
    let queue = Queue(limit: 10)
    admit(queue)
    return next(queue)
}

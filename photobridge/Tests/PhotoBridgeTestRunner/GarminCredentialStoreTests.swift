import PhotoBridgeLib

private final class FakeGarminCredentialStore: GarminCredentialStore {
    private var passwords: [String: String] = [:]
    private(set) var reads: [String] = []
    private(set) var writes: [String] = []

    func password(for account: String) throws -> String? {
        reads.append(account)
        return passwords[account]
    }

    func save(password: String, for account: String) throws {
        writes.append(account)
        passwords[account] = password
    }
}

func runGarminCredentialStoreTests() {
    suite("Garmin credential store contract") {
        test("fake store keeps credentials scoped to their Garmin account") {
            let store = FakeGarminCredentialStore()
            try store.save(password: "first", for: "first@example.invalid")
            try store.save(password: "second", for: "second@example.invalid")
            try expect(try store.password(for: "first@example.invalid"), equals: "first")
            try expect(try store.password(for: "second@example.invalid"), equals: "second")
            try expect(store.writes, equals: ["first@example.invalid", "second@example.invalid"])
        }

        test("keychain rejects blank account and password before touching macOS storage") {
            let keychain = GarminKeychain(service: "io.picmanager.garmin.test")
            do {
                try keychain.save(password: "value", for: "   ")
                throw TestFailure.conditionFailed("blank account was accepted")
            } catch GarminKeychainError.emptyAccount {}
            do {
                try keychain.save(password: "", for: "fixture@example.invalid")
                throw TestFailure.conditionFailed("blank password was accepted")
            } catch GarminKeychainError.emptyPassword {}
        }
    }
}

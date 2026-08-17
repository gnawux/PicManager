import Foundation
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

        test("service startup never reads Keychain before an explicit activation") {
            let store = FakeGarminCredentialStore()
            try store.save(password: "secret", for: "runner@example.invalid")
            let activation = GarminCredentialActivation(store: store)

            try expect(activation.passwordForService(account: "runner@example.invalid"), equals: nil)
            try expect(store.reads, equals: [])
            try expect(try activation.activateSavedCredential(for: "runner@example.invalid"), equals: true)
            try expect(store.reads, equals: ["runner@example.invalid"])
            try expect(activation.passwordForService(account: "runner@example.invalid"), equals: "secret")
            try expect(store.reads, equals: ["runner@example.invalid"])
        }

        test("native preparation result uses the literal snake-case wire contract") {
            let request = GarminPreparationRequest(wireObject: [
                "action": "prepareGarmin",
                "request_id": "request-42",
                "operation": "sync",
                "future_field": true,
            ])
            try expect(request?.requestID, equals: "request-42")
            try expect(request?.operation, equals: .sync)
            try expect(GarminPreparationRequest(wireObject: [
                "action": "prepareGarmin",
                "requestID": "wrong-casing",
                "operation": "sync",
            ]), equals: nil)

            let data = try JSONEncoder().encode(
                GarminPreparationResult.ready(requestID: "request-42")
            )
            let object = try JSONSerialization.jsonObject(with: data) as? [String: Any]
            try expect(object?["request_id"] as? String, equals: "request-42")
            try expect(object?["outcome"] as? String, equals: "ready")
            try expect(object?["requestID"] as? String, equals: nil)
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

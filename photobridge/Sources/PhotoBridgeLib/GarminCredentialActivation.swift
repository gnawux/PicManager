import Foundation

public struct GarminPreparationRequest: Equatable {
    public enum Operation: String {
        case authenticate
        case sync
    }

    public let requestID: String
    public let operation: Operation

    public init?(wireObject: [String: Any]) {
        guard wireObject["action"] as? String == "prepareGarmin",
              let requestID = wireObject["request_id"] as? String,
              !requestID.isEmpty,
              let operationValue = wireObject["operation"] as? String,
              let operation = Operation(rawValue: operationValue) else { return nil }
        self.requestID = requestID
        self.operation = operation
    }
}

/// Keeps Keychain access behind an explicit Garmin action. Service startup may ask
/// for an already activated password, but that lookup never touches persistent storage.
public final class GarminCredentialActivation {
    private let store: any GarminCredentialStore
    private var activeAccount: String?
    private var activePassword: String?

    public init(store: any GarminCredentialStore) {
        self.store = store
    }

    public func passwordForService(account: String?) -> String? {
        guard let account = normalized(account), account == activeAccount else { return nil }
        return activePassword
    }

    /// This is the only read path and must be called in response to an explicit
    /// authenticate or synchronize action from the activity UI.
    public func activateSavedCredential(for account: String) throws -> Bool {
        let account = try requiredAccount(account)
        guard let password = try store.password(for: account) else { return false }
        activeAccount = account
        activePassword = password
        return true
    }

    public func saveAndActivate(password: String, for account: String) throws {
        let account = try requiredAccount(account)
        try store.save(password: password, for: account)
        activeAccount = account
        activePassword = password
    }

    public func deactivate() {
        activeAccount = nil
        activePassword = nil
    }

    private func normalized(_ account: String?) -> String? {
        guard let account else { return nil }
        let value = account.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return value.isEmpty ? nil : value
    }

    private func requiredAccount(_ account: String) throws -> String {
        guard let account = normalized(account) else { throw GarminKeychainError.emptyAccount }
        return account
    }
}

public struct GarminPreparationResult: Codable, Equatable {
    public let requestID: String
    public let outcome: String
    public let message: String?

    public init(requestID: String, outcome: String, message: String? = nil) {
        self.requestID = requestID
        self.outcome = outcome
        self.message = message
    }

    public static func ready(requestID: String) -> Self {
        Self(requestID: requestID, outcome: "ready")
    }

    public static func cancelled(requestID: String) -> Self {
        Self(requestID: requestID, outcome: "cancelled")
    }

    public static func error(requestID: String, message: String) -> Self {
        Self(requestID: requestID, outcome: "error", message: message)
    }

    enum CodingKeys: String, CodingKey {
        case requestID = "request_id"
        case outcome
        case message
    }
}

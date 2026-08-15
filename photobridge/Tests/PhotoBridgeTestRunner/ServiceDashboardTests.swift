import Foundation
import PhotoBridgeLib

func runServiceDashboardTests() {
    suite("Native service dashboard") {
        test("decodes the stable service contract") {
            let data = Data(#"{"api_version":"v1","service_version":"0.1.0","minimum_client_api":"v1","local_trusted_only":true,"capabilities":["health"]}"#.utf8)
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            let contract = try decoder.decode(ServiceContract.self, from: data)
            try expect(contract.apiVersion, equals: "v1")
            try expect(contract.localTrustedOnly)
        }

        test("summarizes Apple synchronization and background health") {
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            let contract = try decoder.decode(
                ServiceContract.self,
                from: Data(#"{"api_version":"v1","service_version":"0.1.0","minimum_client_api":"v1","local_trusted_only":true,"capabilities":[]}"#.utf8)
            )
            let health = try decoder.decode(
                ServiceHealth.self,
                from: Data(#"{"status":"healthy","checked_at":"2026-08-15T00:00:00Z","sqlite_quick_check":"ok","journal_mode":"wal","schema_version":26,"application_jobs_queued":2,"application_jobs_running":1,"application_jobs_failed":0,"sync_jobs_failed":1}"#.utf8)
            )
            let metrics = try decoder.decode(
                WorkerMetrics.self,
                from: Data(#"{"queued":2,"running":1,"retry_wait":1,"succeeded":20,"failed":2,"cancelled":0}"#.utf8)
            )
            let dashboard = ServiceDashboard(
                contract: contract,
                health: health,
                metrics: metrics,
                tasks: [],
                sourceStatusCounts: ["synced": 80, "discovered": 12, "excluded": 3]
            )
            try expect(dashboard.synchronizedSourceCount, equals: 80)
            try expect(dashboard.pendingSourceCount, equals: 12)
            try expect(dashboard.failedWorkCount, equals: 3)
        }
    }
}

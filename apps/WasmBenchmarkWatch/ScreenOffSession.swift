// Keeps the watch app running while the screen is off during a run: a
// WKExtendedRuntimeSession of the type WKBackgroundModes declares
// (mindfulness: the app stays the frontmost app, up to an hour per session).
//
// A session can only start while the app is active. It ends when its hour is
// up, when the person leaves the app (Digital Crown, another app), or when the
// system cancels it because the app's CPU use over a 60-second window went past
// the session's limit. While a session runs, the screen can turn off: the scene
// is then inactive, not in the background, and BenchmarkSession counts it as
// the foreground. Without a session, a run pauses once the scene is no longer
// active and measures the interrupted benchmark again.

import Foundation
import WatchKit

@MainActor
final class ScreenOffSession: NSObject {
    /// True while a session keeps the app running.
    private(set) var isRunning = false
    /// Called on the main actor when `isRunning` changes.
    var onChange: (@MainActor (_ running: Bool) -> Void)?

    /// The one session this object started and has not seen end. Kept until
    /// its didInvalidate arrives (the session's `delegate` is weak, and a
    /// session that is dropped while it starts could keep running unowned).
    private var session: WKExtendedRuntimeSession?
    /// Whether a run wants a session. A session that starts after stop() is
    /// invalidated at once.
    private var wanted = false
    /// False once a start failed with notApprovedToStartSession (a build
    /// without WKBackgroundModes): no more attempts in this process.
    private var approved = true

    /// Starts a session unless one is starting or running. Call it only while
    /// the scene is active; otherwise the session ends at once with
    /// mustBeActiveToStartOrSchedule.
    func start() {
        wanted = true
        guard approved else { return }
        if let session {
            switch session.state {
            case .running:
                update(running: true)
                return
            case .notStarted, .scheduled:
                return  // didStart or didInvalidate is on its way
            case .invalid:
                break
            @unknown default:
                return
            }
        }
        let session = WKExtendedRuntimeSession()
        session.delegate = self
        self.session = session
        session.start()
    }

    /// Ends the session at the end of a run. invalidate() works in any app
    /// state for a session started with start().
    func stop() {
        wanted = false
        update(running: false)
        // A session that has not started yet is ended in didStart: invalidate()
        // before it runs only reports notYetStarted.
        if let session, session.state == .running { session.invalidate() }
    }

    private func update(running: Bool) {
        guard running != isRunning else { return }
        isRunning = running
        onChange?(running)
    }

    private func current(_ id: ObjectIdentifier) -> WKExtendedRuntimeSession? {
        guard let session, ObjectIdentifier(session) == id else { return nil }
        return session
    }

    fileprivate func didStart(_ id: ObjectIdentifier) {
        guard let session = current(id) else { return }
        guard wanted else {
            session.invalidate()
            return
        }
        let until = session.expirationDate.map { $0.formatted(date: .omitted, time: .standard) } ?? "?"
        consoleLog("screen-off session: running until \(until)")
        update(running: true)
    }

    fileprivate func willExpire(_ id: ObjectIdentifier) {
        guard current(id) != nil else { return }
        consoleLog("screen-off session: about to expire")
    }

    fileprivate func didInvalidate(_ id: ObjectIdentifier, reason: WKExtendedRuntimeSessionInvalidationReason, errorCode: Int?) {
        guard current(id) != nil else { return }
        session = nil
        if reason == .error, errorCode == WKExtendedRuntimeSessionErrorCode.notApprovedToStartSession.rawValue {
            approved = false
        }
        consoleLog("screen-off session ended: \(Self.describe(reason, errorCode: errorCode))")
        update(running: false)
    }

    private static func describe(_ reason: WKExtendedRuntimeSessionInvalidationReason, errorCode: Int?) -> String {
        switch reason {
        case .none: return "stopped"
        case .sessionInProgress: return "another session is running"
        case .expired: return "time limit reached"
        case .resignedFrontmost: return "left the app"
        case .suppressedBySystem: return "not allowed in the watch's current state"
        case .error:
            switch errorCode.flatMap(WKExtendedRuntimeSessionErrorCode.init(rawValue:)) {
            case .exceededResourceLimits: return "CPU limit exceeded"
            case .mustBeActiveToStartOrSchedule: return "the app was not active"
            case .notApprovedToStartSession: return "no session type in WKBackgroundModes"
            case .notYetStarted: return "stopped before it started"
            default: return "error \(errorCode.map(String.init) ?? "?")"
            }
        @unknown default: return "reason \(reason.rawValue)"
        }
    }
}

// The SDK names no queue for these callbacks: hop to the main queue and pass
// only Sendable values.
extension ScreenOffSession: WKExtendedRuntimeSessionDelegate {
    nonisolated func extendedRuntimeSessionDidStart(_ extendedRuntimeSession: WKExtendedRuntimeSession) {
        let id = ObjectIdentifier(extendedRuntimeSession)
        DispatchQueue.main.async { MainActor.assumeIsolated { self.didStart(id) } }
    }

    nonisolated func extendedRuntimeSessionWillExpire(_ extendedRuntimeSession: WKExtendedRuntimeSession) {
        let id = ObjectIdentifier(extendedRuntimeSession)
        DispatchQueue.main.async { MainActor.assumeIsolated { self.willExpire(id) } }
    }

    nonisolated func extendedRuntimeSession(
        _ extendedRuntimeSession: WKExtendedRuntimeSession,
        didInvalidateWith reason: WKExtendedRuntimeSessionInvalidationReason,
        error: (any Error)?
    ) {
        let id = ObjectIdentifier(extendedRuntimeSession)
        let code = (error as NSError?).flatMap { $0.domain == WKExtendedRuntimeSessionErrorDomain ? $0.code : nil }
        DispatchQueue.main.async { MainActor.assumeIsolated { self.didInvalidate(id, reason: reason, errorCode: code) } }
    }
}

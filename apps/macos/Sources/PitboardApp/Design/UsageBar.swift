import PitboardKit
import SwiftUI

/// An account's limits, a row each: its name, how much of it is used, and when it comes
/// back.
///
/// A grid, so the columns line up across rows and the name column is as wide as its widest
/// name: a limit scoped to one model is named "week · Sonnet 4.5", which no fixed width
/// holds at every text size. The time left moves on once a minute, which is as often as
/// "resets in 2h 10m" changes.
struct UsageBars: View {
    let limits: [Limit]

    var body: some View {
        TimelineView(.everyMinute) { context in
            Grid(
                alignment: .leading, horizontalSpacing: Design.iconSpacing,
                verticalSpacing: Design.rowSpacing
            ) {
                ForEach(Array(limits.enumerated()), id: \.offset) { _, window in
                    UsageBar(window: window, now: context.date)
                }
            }
            .font(.callout)
        }
    }
}

/// One limit, as a row of `UsageBars`: the platform's capacity bar, tinted by how much is
/// used, with the name and the figures beside it. VoiceOver hears the row as one sentence,
/// read from its name, and not the bar and each figure on their own.
struct UsageBar: View {
    let window: Limit
    let now: Date
    @ScaledMetric(relativeTo: .callout) private var percentWidth: CGFloat = 38
    @ScaledMetric(relativeTo: .callout) private var resetsWidth: CGFloat = 110

    var body: some View {
        GridRow {
            Text(limitColumn(limit: window))
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .accessibilityLabel(spokenLimit(window, resettingIn: secondsLeft))
            Gauge(value: min(max(window.percent, 0), 100), in: 0...100) {
                EmptyView()
            }
            .gaugeStyle(.linearCapacity)
            .tint(usageLevel(percent: window.percent).tint)
            .accessibilityHidden(true)
            Text("\(Int(window.percent.rounded()))%")
                .monospacedDigit()
                .frame(minWidth: percentWidth, alignment: .trailing)
                .gridColumnAlignment(.trailing)
                .accessibilityHidden(true)
            Text(resetText(window, at: now))
                .foregroundStyle(.secondary)
                .monospacedDigit()
                .frame(minWidth: resetsWidth, alignment: .trailing)
                .gridColumnAlignment(.trailing)
                .accessibilityHidden(true)
        }
    }

    private var secondsLeft: TimeInterval? {
        window.resetsAt.map {
            Date(timeIntervalSince1970: TimeInterval($0)).timeIntervalSince(now)
        }
    }
}

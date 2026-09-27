import PitboardKit
import SwiftUI

/// One limit: its name, how much of it is used, and when it comes back.
///
/// The platform's own bar, tinted by how much is used. The columns line up across rows,
/// which needs fixed widths, and each width scales with the text size so larger text never
/// clips. The row says its name and figures beside the bar, and says all of it to VoiceOver
/// in one sentence.
struct UsageBar: View {
    let window: Limits
    @ScaledMetric(relativeTo: .callout) private var nameWidth: CGFloat = 64
    @ScaledMetric(relativeTo: .callout) private var percentWidth: CGFloat = 38
    @ScaledMetric(relativeTo: .callout) private var resetsWidth: CGFloat = 70

    var body: some View {
        // Once a minute is as often as "in 2h 10m" changes.
        TimelineView(.everyMinute) { context in
            HStack(spacing: Design.iconSpacing) {
                Text(windowShortName(window))
                    .foregroundStyle(.secondary)
                    .frame(width: nameWidth, alignment: .leading)
                Gauge(value: min(max(window.percent, 0), 100), in: 0...100) {
                    EmptyView()
                }
                .gaugeStyle(.linearCapacity)
                .tint(UsageLevel(percent: window.percent).tint)
                Text("\(Int(window.percent.rounded()))%")
                    .monospacedDigit()
                    .frame(width: percentWidth, alignment: .trailing)
                Text(resets(at: context.date) ?? "")
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
                    .frame(width: resetsWidth, alignment: .trailing)
            }
            .font(.callout)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(spokenLimit(window, resettingIn: seconds(at: context.date)))
        }
    }

    private func seconds(at now: Date) -> TimeInterval? {
        window.resetsAt.map {
            Date(timeIntervalSince1970: TimeInterval($0)).timeIntervalSince(now)
        }
    }

    private func resets(at now: Date) -> String? {
        seconds(at: now).flatMap(resetsIn)
    }
}

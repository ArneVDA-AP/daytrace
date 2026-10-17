//
//  ReplayHarnessTests.swift
//  Daytrace reference harness (not part of Dayflow).
//
//  Copied into Dayflow/DayflowTests by .github/workflows/dayflow-reference.yml in the
//  Daytrace repository. It replays a folder of synthetic JPEG frames through Dayflow's real
//  OpenAI-compatible analysis path (OpenAICompatibleProvider: transcribeScreenshots, then
//  generateActivityCards with the 45-minute lookback window) and writes every observation,
//  card and request payload as JSON, so Daytrace (the Windows port) can be compared with it.
//
//  The batch driver is a copy of LLMService.processBatch and AnalysisManager's batching, kept
//  in memory instead of in Dayflow's SQLite store: the hosted app starts its own analysis job
//  at launch, and that job must not see these frames. Every rule copied here cites its source.
//
//  Environment (passed through xcodebuild as TEST_RUNNER_<NAME>):
//    HARNESS_FRAMES   folder with the JPEGs and manifest.json (required; the test skips without it)
//    HARNESS_OUT      output folder (default: $TMPDIR/dayflow-harness-out)
//    HARNESS_MODE     dry (canned answers, no network) | live (Anthropic Messages API)
//    HARNESS_MODEL    model id (default claude-haiku-5-5)
//    HARNESS_EFFORT   output_config.effort for live mode (default low; "none" leaves it out)
//    HARNESS_BASE_TS  Unix time of the first frame (default: frames end 10 s before now)
//    HARNESS_TZ       time zone for Dayflow's clock strings (default: the process time zone)
//    HARNESS_DAYFLOW_SHA  recorded in run.json
//    ANTHROPIC_API_KEY    live mode only; never written anywhere
//

import AppKit
import Foundation
import ImageIO
import XCTest

@testable import Dayflow

/// JSONSerialization needs NSNull for missing values.
private func orNull(_ value: Any?) -> Any { value ?? NSNull() }

// MARK: - Manifest

private struct HarnessManifest: Decodable {
  struct Frame: Decodable {
    let file: String
    let offset_s: Int
    let idle_s: Int?
  }
  let interval_seconds: Int?
  let frames: [Frame]
}

// MARK: - In-memory copies of Dayflow's tables

private struct HarnessCard {
  let id: Int
  var batchId: Int64
  let card: ActivityCardData
  let startTs: Int
  let endTs: Int
  let day: String
  var isDeleted: Bool
}

private struct HarnessBatch {
  let index: Int
  let id: Int64
  let screenshots: [Screenshot]
  var start: Int { screenshots.first!.capturedAt }
  var end: Int { screenshots.last!.capturedAt }
}

// MARK: - Request recorder and completion closures

private final class HarnessRecorder {
  let mode: String
  let model: String
  let effort: String
  let apiKey: String?
  var sendTemperature = true
  var notes: [String] = []
  var calls: [[String: Any]] = []
  var inputTokens = 0
  var outputTokens = 0
  var batchIndex = -1

  // Dry-mode expectations, set by the driver before each provider call.
  var dryTranscriptDuration = "00:00:01"
  var dryCardStart = ""
  var dryCardEnd = ""

  init(mode: String, model: String, effort: String, apiKey: String?) {
    self.mode = mode
    self.model = model
    self.effort = effort
    self.apiKey = apiKey
  }

  func complete(
    _ request: OpenAICompatibleProvider.ChatRequest, operation: String, batchId: Int64?
  ) async throws -> String {
    var entry: [String: Any] = [
      "seq": calls.count + 1,
      "batch_index": batchIndex,
      "batch_id": orNull(batchId.map { Int($0) }),
      "operation": operation,
      "request": Self.redactedRequest(request),
      "images": Self.imageSummaries(request),
    ]
    let started = Date()
    defer {
      entry["latency_ms"] = Int(Date().timeIntervalSince(started) * 1000)
      calls.append(entry)
    }
    if mode == "live" {
      do {
        let result = try await callAnthropic(request, entry: &entry)
        entry["response_text"] = result
        return result
      } catch {
        entry["error"] = error.localizedDescription
        throw error
      }
    }
    let canned = cannedResponse(operation: operation)
    entry["response_text"] = canned
    entry["dry_run"] = true
    return canned
  }

  // Minimal valid JSON so Dayflow's validators accept it and the pipeline proceeds.
  private func cannedResponse(operation: String) -> String {
    let object: Any
    switch operation {
    case "transcribe_screenshots":
      object = [
        "segments": [
          [
            "start": "00:00:00", "end": dryTranscriptDuration,
            "description":
              "[dry-run] canned observation for batch \(batchIndex + 1), not model output",
          ]
        ]
      ]
    case "generate_cards":
      let card: [String: Any] = [
        "startTime": dryCardStart, "endTime": dryCardEnd, "category": "Work",
        "subcategory": "", "title": "[dry-run] canned card",
        "summary": "[dry-run] canned summary, not model output.", "detailedSummary": "",
        "distractions": [Any](), "appSites": ["primary": NSNull(), "secondary": NSNull()],
      ]
      object = [card]
    default:
      object = [String: Any]()
    }
    let data = try! JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])
    return String(data: data, encoding: .utf8)!
  }

  // Converts Dayflow's OpenAI-style request into a native Anthropic Messages request:
  // system message -> system, text parts -> text blocks, data-URL images -> base64 image blocks.
  func anthropicBody(_ request: OpenAICompatibleProvider.ChatRequest, withTemperature: Bool)
    -> [String: Any]
  {
    var system: [String] = []
    var messages: [[String: Any]] = []
    for message in request.messages {
      if message.role == "system" {
        system.append(contentsOf: message.content.compactMap { $0.text })
        continue
      }
      let blocks: [[String: Any]] = message.content.compactMap { part -> [String: Any]? in
        if part.type == "text", let text = part.text {
          return ["type": "text", "text": text]
        }
        if part.type == "image_url", let url = part.image_url?.url,
          let image = Self.splitDataURL(url)
        {
          return [
            "type": "image",
            "source": ["type": "base64", "media_type": image.mime, "data": image.base64],
          ]
        }
        return nil
      }
      messages.append(["role": message.role, "content": blocks])
    }
    var body: [String: Any] = [
      "model": request.model,
      "max_tokens": request.max_tokens,
      "messages": messages,
    ]
    if !system.isEmpty { body["system"] = system.joined(separator: "\n\n") }
    if withTemperature { body["temperature"] = request.temperature }
    if !effort.isEmpty && effort != "none" { body["output_config"] = ["effort": effort] }
    return body
  }

  private func callAnthropic(
    _ request: OpenAICompatibleProvider.ChatRequest, entry: inout [String: Any]
  ) async throws -> String {
    guard let apiKey, !apiKey.isEmpty else {
      throw NSError(
        domain: "ReplayHarness", code: 1,
        userInfo: [NSLocalizedDescriptionKey: "ANTHROPIC_API_KEY is not set for live mode."])
    }
    for _ in 0..<2 {
      let body = anthropicBody(request, withTemperature: sendTemperature)
      entry["anthropic_request"] = Self.redactJSON(body)
      var urlRequest = URLRequest(url: URL(string: "https://api.anthropic.com/v1/messages")!)
      urlRequest.httpMethod = "POST"
      urlRequest.timeoutInterval = 300
      urlRequest.setValue(apiKey, forHTTPHeaderField: "x-api-key")
      urlRequest.setValue("2023-06-01", forHTTPHeaderField: "anthropic-version")
      urlRequest.setValue("application/json", forHTTPHeaderField: "content-type")
      urlRequest.httpBody = try JSONSerialization.data(withJSONObject: body)
      let (data, response) = try await URLSession.shared.data(for: urlRequest)
      let status = (response as? HTTPURLResponse)?.statusCode ?? 0
      let text = String(data: data, encoding: .utf8) ?? ""
      entry["http_status"] = status
      if status == 400, sendTemperature, text.lowercased().contains("temperature") {
        // Dayflow sends temperature 0.7; drop it once if this model refuses it, and say so.
        sendTemperature = false
        notes.append("API rejected temperature; later requests omit it. Error: \(text.prefix(300))")
        continue
      }
      guard status == 200,
        let json = try JSONSerialization.jsonObject(with: data) as? [String: Any]
      else {
        throw NSError(
          domain: "ReplayHarness", code: status,
          userInfo: [NSLocalizedDescriptionKey: "Anthropic HTTP \(status): \(text.prefix(800))"])
      }
      let blocks = json["content"] as? [[String: Any]] ?? []
      let output = blocks.filter { ($0["type"] as? String) == "text" }
        .compactMap { $0["text"] as? String }.joined()
      let usage = json["usage"] as? [String: Any] ?? [:]
      let input = usage["input_tokens"] as? Int ?? 0
      let outputTokens = usage["output_tokens"] as? Int ?? 0
      inputTokens += input
      self.outputTokens += outputTokens
      entry["usage"] = usage
      entry["stop_reason"] = orNull(json["stop_reason"])
      entry["response_model"] = orNull(json["model"])
      entry["block_types"] = blocks.compactMap { $0["type"] as? String }
      return output
    }
    throw NSError(
      domain: "ReplayHarness", code: 2,
      userInfo: [NSLocalizedDescriptionKey: "Request failed after dropping temperature."])
  }

  // MARK: Redaction helpers

  static func splitDataURL(_ url: String) -> (mime: String, base64: String)? {
    guard url.hasPrefix("data:"), let comma = url.firstIndex(of: ",") else { return nil }
    let header = url[url.index(url.startIndex, offsetBy: 5)..<comma]
    let mime = header.split(separator: ";").first.map(String.init) ?? "image/jpeg"
    return (mime, String(url[url.index(after: comma)...]))
  }

  static func describeImage(base64: String, mime: String) -> [String: Any] {
    guard let data = Data(base64Encoded: base64) else {
      return ["media_type": mime, "base64_chars": base64.count]
    }
    var info: [String: Any] = [
      "media_type": mime, "bytes": data.count, "base64_chars": base64.count,
    ]
    if let source = CGImageSourceCreateWithData(data as CFData, nil),
      let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any]
    {
      info["width"] = orNull(props[kCGImagePropertyPixelWidth])
      info["height"] = orNull(props[kCGImagePropertyPixelHeight])
    }
    return info
  }

  static func imageSummaries(_ request: OpenAICompatibleProvider.ChatRequest) -> [[String: Any]] {
    request.messages.flatMap { $0.content }.compactMap { part -> [String: Any]? in
      guard let url = part.image_url?.url, let image = splitDataURL(url) else { return nil }
      return describeImage(base64: image.base64, mime: image.mime)
    }
  }

  /// The request exactly as Dayflow built it, with each image replaced by its size.
  static func redactedRequest(_ request: OpenAICompatibleProvider.ChatRequest) -> Any {
    guard let data = try? JSONEncoder().encode(request),
      let object = try? JSONSerialization.jsonObject(with: data)
    else { return NSNull() }
    return redactJSON(object)
  }

  static func redactJSON(_ value: Any) -> Any {
    if let dict = value as? [String: Any] {
      var out: [String: Any] = [:]
      for (key, item) in dict {
        if key == "url", let url = item as? String, let image = splitDataURL(url) {
          out[key] =
            "<image " + describeLabel(describeImage(base64: image.base64, mime: image.mime)) + ">"
        } else if key == "data", let base64 = item as? String, base64.count > 512 {
          let mime = (dict["media_type"] as? String) ?? "image/jpeg"
          out[key] = "<image " + describeLabel(describeImage(base64: base64, mime: mime)) + ">"
        } else {
          out[key] = redactJSON(item)
        }
      }
      return out
    }
    if let array = value as? [Any] { return array.map(redactJSON) }
    return value
  }

  static func describeLabel(_ info: [String: Any]) -> String {
    let mime = info["media_type"] as? String ?? "?"
    let bytes = info["bytes"].map { "\($0)" } ?? "?"
    let width = info["width"].map { "\($0)" } ?? "?"
    let height = info["height"].map { "\($0)" } ?? "?"
    return "\(mime) \(bytes) bytes \(width)x\(height)"
  }
}

// MARK: - Test

final class ReplayHarnessTests: XCTestCase {
  private let clockFormatter: DateFormatter = {
    // Same as TimelineOutputSupport.formatTimestampForPrompt.
    let formatter = DateFormatter()
    formatter.dateFormat = "h:mm a"
    formatter.locale = Locale(identifier: "en_US_POSIX")
    return formatter
  }()

  private func clock(_ ts: Int) -> String {
    clockFormatter.timeZone = TimeZone.current
    return clockFormatter.string(from: Date(timeIntervalSince1970: TimeInterval(ts)))
  }

  private func elapsed(_ seconds: Int) -> String {
    // Same as TimelineOutputSupport.formatSeconds.
    String(format: "%02d:%02d:%02d", seconds / 3600, (seconds % 3600) / 60, seconds % 60)
  }

  func testReplay() async throws {
    let env = ProcessInfo.processInfo.environment
    guard let framesPath = env["HARNESS_FRAMES"], !framesPath.isEmpty else {
      throw XCTSkip("HARNESS_FRAMES is not set; the reference harness runs only in its workflow.")
    }
    let mode = (env["HARNESS_MODE"] ?? "dry").lowercased()
    XCTAssertTrue(["dry", "live"].contains(mode), "HARNESS_MODE must be dry or live")
    let model = env["HARNESS_MODEL"].flatMap { $0.isEmpty ? nil : $0 } ?? "claude-haiku-5-5"
    let effort = env["HARNESS_EFFORT"].flatMap { $0.isEmpty ? nil : $0 } ?? "low"
    let outDir = URL(
      fileURLWithPath: env["HARNESS_OUT"].flatMap { $0.isEmpty ? nil : $0 }
        ?? (NSTemporaryDirectory() + "dayflow-harness-out"))
    try FileManager.default.createDirectory(at: outDir, withIntermediateDirectories: true)

    if let tzName = env["HARNESS_TZ"], !tzName.isEmpty {
      let zone = try XCTUnwrap(TimeZone(identifier: tzName), "Unknown HARNESS_TZ \(tzName)")
      setenv("TZ", tzName, 1)
      NSTimeZone.resetSystemTimeZone()
      NSTimeZone.default = zone
    }

    // Frames and timestamps.
    let framesDir = URL(fileURLWithPath: framesPath)
    let manifest = try JSONDecoder().decode(
      HarnessManifest.self,
      from: Data(contentsOf: framesDir.appendingPathComponent("manifest.json")))
    XCTAssertFalse(manifest.frames.isEmpty, "manifest.json lists no frames")
    let lastOffset = manifest.frames.map(\.offset_s).max() ?? 0
    let interval = manifest.interval_seconds ?? 10
    let baseTs =
      env["HARNESS_BASE_TS"].flatMap { Int($0) }
      ?? (Int(Date().timeIntervalSince1970) - lastOffset - interval)
    let screenshots: [Screenshot] = manifest.frames.enumerated().map { index, frame in
      let path = framesDir.appendingPathComponent(frame.file).path
      let size = (try? FileManager.default.attributesOfItem(atPath: path))?[.size] as? Int64
      return Screenshot(
        id: Int64(index + 1), capturedAt: baseTs + frame.offset_s, filePath: path,
        fileSize: size, idleSecondsAtCapture: frame.idle_s ?? 1, isDeleted: false,
        frameIndex: nil)
    }

    // Batching: AnalysisManager.createScreenshotBatches (gap > maxGap or span > target closes
    // a bucket; the trailing bucket is dropped while shorter than the target).
    let config = BatchingConfig.standard
    var buckets: [[Screenshot]] = []
    var bucket: [Screenshot] = []
    for shot in screenshots.sorted(by: { $0.capturedAt < $1.capturedAt }) {
      if let first = bucket.first, let previous = bucket.last {
        let gap = TimeInterval(shot.capturedAt - previous.capturedAt)
        let span = TimeInterval(shot.capturedAt - first.capturedAt)
        if gap > config.maxGap || span > config.targetDuration {
          buckets.append(bucket)
          bucket = []
        }
      }
      bucket.append(shot)
    }
    if !bucket.isEmpty { buckets.append(bucket) }
    var droppedTrailing = 0
    if let last = buckets.last,
      TimeInterval(last.last!.capturedAt - last.first!.capturedAt) < config.targetDuration
    {
      droppedTrailing = last.count
      buckets.removeLast()
    }
    let batches = buckets.enumerated().map { index, shots in
      HarnessBatch(index: index, id: Int64(index + 1), screenshots: shots)
    }

    let apiKey = env["ANTHROPIC_API_KEY"]
    if mode == "live" {
      XCTAssertFalse((apiKey ?? "").isEmpty, "live mode needs ANTHROPIC_API_KEY")
    }
    let recorder = HarnessRecorder(mode: mode, model: model, effort: effort, apiKey: apiKey)
    let configuration = OpenAICompatibleRuntimeConfiguration(
      configuration: .init(
        preset: .custom, baseURL: "https://api.anthropic.com/v1", modelID: model),
      bearerToken: nil)
    let provider = OpenAICompatibleProvider(configuration: configuration) {
      request, operation, batchId in
      try await recorder.complete(request, operation: operation, batchId: batchId)
    }
    let categories = CategoryStore.descriptorsForLLM()

    var observations: [Observation] = []
    var nextObservationId: Int64 = 1
    var cards: [HarnessCard] = []
    var nextCardId = 1
    var batchRecords: [[String: Any]] = []
    var perBatchCards: [[String: Any]] = []
    let startedAt = Date()

    for batch in batches {
      recorder.batchIndex = batch.index
      var record: [String: Any] = [
        "index": batch.index, "id": Int(batch.id), "start_ts": batch.start,
        "end_ts": batch.end, "start_clock": clock(batch.start), "end_clock": clock(batch.end),
        "frames": batch.screenshots.count,
      ]
      let callsBefore = recorder.calls.count

      // Gates from AnalysisManager.queueLLMRequest.
      let span = batch.end - batch.start
      if span < 300 {
        record["status"] = "skipped_short"
        batchRecords.append(record)
        continue
      }
      if IdleBatchClassifier.assess(batch.screenshots) != nil {
        // Simplified idle shortcut: fixed texts from IdleBatchClassifier.makeIdleCard,
        // without extending a previous Idle card.
        let idle = ActivityCardData(
          startTime: clock(batch.start), endTime: clock(batch.end), category: "Idle",
          subcategory: "", title: "Idle", summary: "You were idle during this period.",
          detailedSummary: "Idle period. Dayflow skipped activity summarization for this block.",
          distractions: nil, appSites: nil)
        replaceCards(
          &cards, nextId: &nextCardId, from: batch.start, to: batch.end, with: [idle],
          batchId: batch.id)
        record["status"] = "analyzed"
        record["reason"] = "idle_shortcut_applied"
        batchRecords.append(record)
        continue
      }

      // LLMService.processBatch (two-step path used by the OpenAI-compatible provider).
      let currentTime = batch.end
      let windowStart = currentTime - Int(config.cardLookbackDuration)
      do {
        recorder.dryTranscriptDuration = elapsed(max(1, span))
        let transcribed = try await provider.transcribeScreenshots(
          batch.screenshots, batchStartTime: Date(timeIntervalSince1970: TimeInterval(batch.start)),
          batchId: batch.id)
        let saved = transcribed.observations.map { obs -> Observation in
          defer { nextObservationId += 1 }
          return Observation(
            id: nextObservationId, batchId: batch.id, startTs: obs.startTs, endTs: obs.endTs,
            observation: obs.observation, metadata: obs.metadata, llmModel: obs.llmModel,
            createdAt: obs.createdAt)
        }
        observations.append(contentsOf: saved)
        record["observations"] = saved.count
        guard !saved.isEmpty else {
          record["status"] = "analyzed"
          record["reason"] = "transcription returned 0 observations"
          record["requests"] = recorder.calls.count - callsBefore
          batchRecords.append(record)
          continue
        }

        // StorageManager.fetchObservationsByTimeRange(windowStart, currentTime).
        let recent = observations.filter {
          ($0.startTs < currentTime && $0.endTs > windowStart)
            || ($0.startTs >= windowStart && $0.startTs < currentTime)
        }.sorted { $0.startTs < $1.startTs }
        // fetchTimelineCardsByTimeRange minus System cards, converted to ActivityCardData.
        let windowCards = cards.filter {
          !$0.isDeleted
            && (($0.startTs < currentTime && $0.endTs > windowStart)
              || ($0.startTs >= windowStart && $0.startTs < currentTime))
        }.sorted { ($0.startTs, $0.id) < ($1.startTs, $1.id) }
        let existing = windowCards.filter { $0.card.category != "System" }.map(\.card)
        // StorageManager.hasTimelineCardConnected(toBatchStartingAt:maxGap: 300).
        let connected = cards.contains {
          !$0.isDeleted && $0.card.category != "System" && $0.startTs <= batch.start
            && $0.endTs >= batch.start - 300
        }
        let context = ActivityGenerationContext(
          batchObservations: saved, existingCards: existing,
          currentTime: Date(timeIntervalSince1970: TimeInterval(currentTime)),
          categories: categories, hasPreviousCardWithinFiveMinutes: connected)

        let existingStarts = windowCards.filter { $0.card.category != "System" }.map(\.startTs)
        let existingEnds = windowCards.filter { $0.card.category != "System" }.map(\.endTs)
        recorder.dryCardStart = clock(
          min(recent.map(\.startTs).min() ?? batch.start, existingStarts.min() ?? Int.max))
        recorder.dryCardEnd = clock(
          max(recent.map(\.endTs).max() ?? batch.end, existingEnds.max() ?? Int.min))

        let generated = try await provider.generateActivityCards(
          observations: recent, context: context, batchId: batch.id)
        // Non-Claude providers replace [windowStart, batchEnd] (LLMService.cardReplacement*Time).
        let inserted = replaceCards(
          &cards, nextId: &nextCardId, from: windowStart, to: currentTime,
          with: generated.cards, batchId: batch.id)
        perBatchCards.append([
          "batch_index": batch.index, "window_start_ts": windowStart,
          "window_end_ts": currentTime, "recent_observations": recent.count,
          "existing_cards_in_prompt": existing.count,
          "cards": inserted.map { cardJSON($0, baseTs: baseTs) },
        ])
        record["status"] = "analyzed"
        record["cards_returned"] = generated.cards.count
      } catch {
        // LLMService failure path: a System error card replaces [batchStart, batchEnd].
        let failure = ActivityCardData(
          startTime: clock(batch.start), endTime: clock(batch.end), category: "System",
          subcategory: "Error", title: "Processing failed",
          summary: error.localizedDescription + " Your recording is saved.", detailedSummary: "",
          distractions: nil, appSites: nil)
        replaceCards(
          &cards, nextId: &nextCardId, from: batch.start, to: batch.end, with: [failure],
          batchId: batch.id)
        record["status"] = "failed"
        record["reason"] = error.localizedDescription
      }
      record["requests"] = recorder.calls.count - callsBefore
      batchRecords.append(record)
    }

    // Output.
    let real = mode == "live"
    let label: [String: Any] = [
      "mode": mode, "real_model_output": real,
      "note": real
        ? "Live run: \(model) via the Anthropic Messages API."
        : "DRY RUN: every response is canned by the harness. This is NOT model output.",
    ]
    func wrap(_ items: Any) -> [String: Any] {
      var out = label
      out["items"] = items
      return out
    }
    let observationItems: [[String: Any]] = observations.map { obs -> [String: Any] in
      [
        "batch_id": Int(obs.batchId), "start_ts": obs.startTs, "end_ts": obs.endTs,
        "start_clock": clock(obs.startTs), "end_clock": clock(obs.endTs),
        "offset_start_s": obs.startTs - baseTs, "offset_end_s": obs.endTs - baseTs,
        "observation": obs.observation, "llm_model": orNull(obs.llmModel),
      ]
    }
    let finalCards = cards.filter { !$0.isDeleted }
      .sorted { ($0.startTs, $0.id) < ($1.startTs, $1.id) }
    var cardsOut = label
    cardsOut["final"] = finalCards.map { cardJSON($0, baseTs: baseTs) }
    cardsOut["per_batch"] = perBatchCards
    var batchesOut = label
    batchesOut["batches"] = batchRecords
    batchesOut["dropped_trailing_frames"] = droppedTrailing

    let finishedAt = Date()
    let iso = ISO8601DateFormatter()
    // Claude Haiku 5.5 list price, prompts up to 100k tokens: $0.10 / $0.50 per million.
    var costEstimate: Double? = nil
    if real && model == "claude-haiku-5-5" {
      costEstimate =
        (Double(recorder.inputTokens) * 0.10 + Double(recorder.outputTokens) * 0.50) / 1_000_000
    }
    // Assigned one by one: a single mixed-type literal is slow for the Swift type checker.
    var run = label
    run["dayflow_sha"] = orNull(env["HARNESS_DAYFLOW_SHA"])
    run["dayflow_path"] =
      "OpenAICompatibleProvider (two-step: transcribe_screenshots, generate_cards)"
    run["model"] = model
    run["effort"] = orNull(real ? effort : nil)
    run["temperature_sent"] = orNull(real ? recorder.sendTemperature : nil)
    run["time_zone"] = TimeZone.current.identifier
    run["base_ts"] = baseTs
    run["frames"] = screenshots.count
    run["batches"] = batches.count
    run["started_at"] = iso.string(from: startedAt)
    run["finished_at"] = iso.string(from: finishedAt)
    run["duration_s"] = Int(finishedAt.timeIntervalSince(startedAt))
    run["requests"] = recorder.calls.count
    run["usage"] = ["input_tokens": recorder.inputTokens, "output_tokens": recorder.outputTokens]
    run["cost_estimate_usd"] = orNull(costEstimate)
    run["categories"] = categories.map(\.name)
    run["notes"] = recorder.notes

    try writeJSON(run, to: outDir.appendingPathComponent("run.json"))
    try writeJSON(batchesOut, to: outDir.appendingPathComponent("batches.json"))
    try writeJSON(wrap(observationItems), to: outDir.appendingPathComponent("observations.json"))
    try writeJSON(cardsOut, to: outDir.appendingPathComponent("cards.json"))
    try writeJSON(wrap(recorder.calls), to: outDir.appendingPathComponent("requests.json"))

    print("REPLAY HARNESS mode=\(mode) batches=\(batches.count) observations=\(observations.count)")
    for card in finalCards {
      print("  \(card.card.startTime) - \(card.card.endTime) [\(card.card.category)] \(card.card.title)")
    }
    print("  tokens in=\(recorder.inputTokens) out=\(recorder.outputTokens) out_dir=\(outDir.path)")

    XCTAssertFalse(batches.isEmpty, "no complete 15-minute batch in the frames")
    for record in batchRecords {
      XCTAssertNotEqual(
        record["status"] as? String, "failed",
        "batch \(record["index"] ?? "?") failed: \(record["reason"] ?? "")")
    }
  }

  // MARK: - StorageManager.replaceTimelineCardsInRange, in memory

  @discardableResult
  private func replaceCards(
    _ cards: inout [HarnessCard], nextId: inout Int, from: Int, to: Int,
    with newCards: [ActivityCardData], batchId: Int64
  ) -> [HarnessCard] {
    for index in cards.indices where !cards[index].isDeleted {
      let card = cards[index]
      let overlaps =
        (card.startTs < to && card.endTs > from) || (card.startTs >= from && card.startTs < to)
      if overlaps && (card.card.category != "System" || card.batchId == batchId) {
        cards[index].isDeleted = true
      }
    }
    let parser = DateFormatter()
    parser.dateFormat = "h:mm a"
    parser.locale = Locale(identifier: "en_US_POSIX")
    let calendar = Calendar.current
    let anchor = Date(timeIntervalSince1970: TimeInterval(from) + TimeInterval(to - from) / 2.0)
    func resolve(_ hour: Int, _ minute: Int) -> Date {
      guard let sameDay = calendar.date(bySettingHour: hour, minute: minute, second: 0, of: anchor)
      else { return anchor }
      let previousDay = calendar.date(byAdding: .day, value: -1, to: sameDay) ?? sameDay
      let nextDay = calendar.date(byAdding: .day, value: 1, to: sameDay) ?? sameDay
      return [previousDay, sameDay, nextDay].min {
        abs($0.timeIntervalSince(anchor)) < abs($1.timeIntervalSince(anchor))
      } ?? sameDay
    }
    var inserted: [HarnessCard] = []
    for card in newCards {
      // Unparsable clock strings are skipped silently, as in Dayflow.
      guard let startTime = parser.date(from: card.startTime),
        let endTime = parser.date(from: card.endTime)
      else { continue }
      let start = calendar.dateComponents([.hour, .minute], from: startTime)
      let end = calendar.dateComponents([.hour, .minute], from: endTime)
      guard let sh = start.hour, let sm = start.minute, let eh = end.hour, let em = end.minute
      else { continue }
      let startDate = resolve(sh, sm)
      var endDate = resolve(eh, em)
      if endDate < startDate {
        endDate = calendar.date(byAdding: .day, value: 1, to: endDate) ?? endDate
      }
      let stored = HarnessCard(
        id: nextId, batchId: batchId, card: card,
        startTs: Int(startDate.timeIntervalSince1970), endTs: Int(endDate.timeIntervalSince1970),
        day: startDate.getDayInfoFor4AMBoundary().dayString, isDeleted: false)
      nextId += 1
      cards.append(stored)
      inserted.append(stored)
    }
    return inserted
  }

  private func cardJSON(_ stored: HarnessCard, baseTs: Int) -> [String: Any] {
    var out: [String: Any] = [
      "id": stored.id, "batch_id": Int(stored.batchId), "start_ts": stored.startTs,
      "end_ts": stored.endTs, "offset_start_s": stored.startTs - baseTs,
      "offset_end_s": stored.endTs - baseTs, "day": stored.day,
    ]
    if let data = try? JSONEncoder().encode(stored.card),
      let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
    {
      out["card"] = object
    }
    return out
  }

  private func writeJSON(_ object: Any, to url: URL) throws {
    let data = try JSONSerialization.data(
      withJSONObject: object, options: [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes])
    try data.write(to: url, options: .atomic)
  }
}

import { render, screen } from "@testing-library/react";
import type { ReviewedInterpretation, Value } from "../api/types";
import { ReviewedReading, ReviewedDetails } from "../components/ReviewedReading";

const reading: ReviewedInterpretation = {
  setting_id: "power.limit", label: "Power limit",
  value: { kind: "integer", value: 235, unit: "W" },
  default_value: null, baseline_value: null, differs_from_default: null,
  evidence: [], components: [],
};

it("keeps physical values and unknown defaults, qualifying missing evidence conservatively", () => {
  render(<><ReviewedReading reading={reading} source="saved" /><ReviewedReading reading={reading} source="default" /><ReviewedDetails reading={reading} /></>);
  expect(screen.getAllByText("235 W").length).toBeGreaterThan(0);
  expect(screen.getByText("Unknown")).toBeInTheDocument();
  expect(screen.getAllByText("Recipe reading (not checked on this board):")).toHaveLength(2);
  expect(screen.getByText(/This recipe has not been checked on this board/)).toBeInTheDocument();
});

it.each(["examined", "untested"] as const)("uses the %s evidence qualifier", board_evidence => {
  render(<ReviewedReading reading={{ ...reading, board_evidence }} source="saved" />);
  expect(screen.getByText(board_evidence === "examined" ? "Reviewed reading:" : "Recipe reading (not checked on this board):")).toBeInTheDocument();
});

it.each<Value>([
  { kind: "enum", symbol: "auto", label: "Auto" },
  { kind: "enum", symbol: "compatible", label: "Compatible Mode" },
  { kind: "boolean", value: true },
  { kind: "integer", value: 2, unit: "raw" },
])("suppresses plain labels and aliases inline: %j", value => {
  const { container } = render(<ReviewedReading reading={{ ...reading, value }} source="saved" />);
  expect(container).toBeEmptyDOMElement();
});

it("suppresses an identical physical reading already shown", () => {
  const { container } = render(<ReviewedReading reading={reading} source="saved" originalText="235 W" />);
  expect(container).toBeEmptyDOMElement();
});

it.each<Value>([
  { kind: "integer", value: -100, unit: "mV" },
  { kind: "integer", value: 2, unit: "ratio" },
  { kind: "auto" },
])("retains signed values and composite Auto: %j", value => {
  render(<ReviewedReading reading={{ ...reading, value, default_value: { kind: "integer", value: 2, unit: "ratio" } }} source="saved" />);
  expect(screen.getByText(value.kind === "auto" ? "Auto" : value.kind === "integer" && value.unit === "ratio" ? "+2 bins" : "-100 mV")).toBeInTheDocument();
});

it("keeps recipe enum aliases in explicitly qualified details", () => {
  render(<ReviewedDetails reading={{ ...reading, value: { kind: "enum", symbol: "compatible", label: "Compatible Mode" } }} />);
  expect(screen.getByText("Power limit: Compatible Mode")).toBeInTheDocument();
  expect(screen.getByText(/separately from the BIOS label and may use different aliases/)).toBeInTheDocument();
});

it("adds nothing when no interpretation exists", () => {
  const { container } = render(<ReviewedReading reading={undefined} source="saved" />);
  expect(container).toBeEmptyDOMElement();
});

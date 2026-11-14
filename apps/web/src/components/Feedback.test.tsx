import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ApiError } from "../api/client";
import { CopyProvider } from "../copy/CopyProvider";
import en from "../copy/en";
import ja from "../copy/ja";
import { ErrorMessage } from "./Feedback";

const RAW_SERVER_MESSAGE = "raw server detail: limiter bucket 10.0.0.7 exhausted";

describe("ErrorMessage", () => {
  it("shows the rate_limited copy, never the raw server message", () => {
    render(
      <CopyProvider locale="en">
        <ErrorMessage error={new ApiError("rate_limited", 429, RAW_SERVER_MESSAGE)} />
      </CopyProvider>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(en.errors.rate_limited);
    expect(screen.queryByText(RAW_SERVER_MESSAGE, { exact: false })).toBeNull();
  });

  it("shows the payload_too_large copy, never the raw server message", () => {
    render(
      <CopyProvider locale="en">
        <ErrorMessage error={new ApiError("payload_too_large", 413, RAW_SERVER_MESSAGE)} />
      </CopyProvider>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(en.errors.payload_too_large);
    expect(screen.queryByText(RAW_SERVER_MESSAGE, { exact: false })).toBeNull();
  });

  it("gives each new code its own copy in Japanese too", () => {
    render(
      <CopyProvider locale="ja">
        <ErrorMessage error={new ApiError("rate_limited", 429, RAW_SERVER_MESSAGE)} />
      </CopyProvider>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(ja.errors.rate_limited);
    expect(ja.errors.rate_limited).not.toBe(ja.errors.payload_too_large);
    expect(en.errors.rate_limited).not.toBe(en.errors.internal);
    expect(en.errors.payload_too_large).not.toBe(en.errors.internal);
    expect(document.body).not.toHaveTextContent(RAW_SERVER_MESSAGE);
  });
});

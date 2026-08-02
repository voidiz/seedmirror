import { z } from "zod";
import { jsonCodec } from "./codecs";

export const FileSyncProgressSchema = z.object({
  remote_file_path: z.string(),
  local_file_path: z.string(),
  transferred: z.string(),
  progress: z.number(),
  transfer_speed: z.string(),
  remaining: z.string(),
});
export type FileSyncProgress = z.infer<typeof FileSyncProgressSchema>;

export const AllProgressPayloadSchema = z.array(FileSyncProgressSchema);
export type AllProgressPayload = z.infer<typeof AllProgressPayloadSchema>;

export const ErrorPayloadSchema = z.object({
  code: z.number().int(),
  message: z.string(),
});
export type ErrorPayload = z.infer<typeof ErrorPayloadSchema>;

export const CurrentStatusResponseSchema = z.object({
  type: z.literal("current_status"),
  data: AllProgressPayloadSchema,
});
export type CurrentStatusResponse = z.infer<typeof CurrentStatusResponseSchema>;

export const ErrorResponseSchema = z.object({
  type: z.literal("error"),
  data: ErrorPayloadSchema,
});
export type ErrorResponse = z.infer<typeof ErrorResponseSchema>;

export const ResponseBodySchema = z.discriminatedUnion("type", [
  CurrentStatusResponseSchema,
  ErrorResponseSchema,
]);
export type ResponseBody = z.infer<typeof ResponseBodySchema>;

export type RequestResponseMap = {
  get_current_status: CurrentStatusResponse;
};

export const WsMessageSchema = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("sync_progress"),
    data: FileSyncProgressSchema,
  }),
  z.object({
    type: z.literal("current_status"),
    data: AllProgressPayloadSchema,
  }),
  z.object({
    type: z.literal("response"),
    data: z.intersection(z.object({ id: z.string() }), ResponseBodySchema),
  }),
  z.object({
    type: z.literal("error"),
    data: ErrorPayloadSchema,
  }),
]);

export type WsMessage = z.infer<typeof WsMessageSchema>;
export const WsMessageJson = jsonCodec(WsMessageSchema);

export const RequestBodySchema = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("get_current_status"),
  }),
]);
export type RequestBody = z.infer<typeof RequestBodySchema>;

export const RequestSchema = z.intersection(
  z.object({
    id: z.string(),
  }),
  RequestBodySchema,
);

export type Request = z.infer<typeof RequestSchema>;
export const RequestJson = jsonCodec(RequestSchema);

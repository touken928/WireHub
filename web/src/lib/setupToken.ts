import { ApiError } from '@/api/http';

export function isSetupTokenRejectedError(err: unknown): err is ApiError {
  return err instanceof ApiError && err.status === 403;
}

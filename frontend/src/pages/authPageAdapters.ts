// Type definitions mirrored from Vue components
export interface AccountRegistrationInput {
  tenantName: string
  username: string
  password: string
}

export interface ResetPasswordInput {
  username: string
  testChallenge: string
  newPassword: string
}

export const authPageAdapters = {
  async registerAccount(_input: AccountRegistrationInput): Promise<void> {
    await Promise.resolve()
  },
  async beginPasswordRecovery(_username: string): Promise<void> {
    await Promise.resolve()
  },
  async completePasswordReset(_input: ResetPasswordInput): Promise<void> {
    await Promise.resolve()
  },
}

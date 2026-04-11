export interface User {
  id: string;
  name: string;
  email: string;
}

export interface Session {
  token: string;
  userId: string;
  expiresAt: number;
}

export function validateUser(user: User): boolean {
  return user.id.length > 0 && user.email.includes("@");
}

export class UserRepository {
  private users: Map<string, User> = new Map();

  findById(id: string): User | undefined {
    return this.users.get(id);
  }

  save(user: User): void {
    this.users.set(user.id, user);
  }

  delete(id: string): boolean {
    return this.users.delete(id);
  }
}

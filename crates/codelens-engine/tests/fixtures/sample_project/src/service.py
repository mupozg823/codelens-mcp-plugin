class UserService:
    """Handles user-related operations."""

    def get_user(self, user_id: str):
        return {"id": user_id, "name": "test"}

    def delete_user(self, user_id: str):
        pass


class AuthService:
    def login(self, username: str, password: str):
        return True

    def logout(self):
        pass


def create_app():
    return UserService()

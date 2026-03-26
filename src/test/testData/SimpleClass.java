public class SimpleClass {
    private int count;
    private String name;

    public SimpleClass(String name) {
        this.name = name;
        this.count = 0;
    }

    public void increment() {
        count++;
    }

    public int getCount() {
        return count;
    }

    public String getName() {
        return name;
    }

    public void setName(String name) {
        this.name = name;
    }

    public static final int MAX_COUNT = 100;
}

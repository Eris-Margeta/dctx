class Job {
    Runnable prepare(int id) {
        final class Step implements Runnable {
            public void run() { System.out.println(id); }
        }
        return new Step();
    }
}
